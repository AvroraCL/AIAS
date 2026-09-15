//! OIDN（Intel Open Image Denoise）动态加载降噪。
//! 使用 oidnSetSharedFilterImage 直接指向 Rust 端内存，无需 OIDN buffer 管理。

use libloading::{Library, Symbol};
use std::{
    ffi::CStr,
    path::{Path, PathBuf},
};

type NewDevice = unsafe extern "C" fn(device_type: i32) -> *mut std::ffi::c_void;
type CommitDevice = unsafe extern "C" fn(device: *mut std::ffi::c_void);
type NewFilter = unsafe extern "C" fn(
    device: *mut std::ffi::c_void,
    type_: *const std::ffi::c_char,
) -> *mut std::ffi::c_void;
type SetSharedFilterImage = unsafe extern "C" fn(
    filter: *mut std::ffi::c_void,
    name: *const std::ffi::c_char,
    dev_ptr: *mut std::ffi::c_void,
    format: i32,
    width: usize,
    height: usize,
    byte_offset: usize,
    pixel_byte_stride: usize,
    row_byte_stride: usize,
);
type SetFilterBool =
    unsafe extern "C" fn(filter: *mut std::ffi::c_void, name: *const std::ffi::c_char, value: bool);
type CommitFilter = unsafe extern "C" fn(filter: *mut std::ffi::c_void);
type ExecuteFilter = unsafe extern "C" fn(filter: *mut std::ffi::c_void);
type ReleaseFilter = unsafe extern "C" fn(filter: *mut std::ffi::c_void);
type ReleaseDevice = unsafe extern "C" fn(device: *mut std::ffi::c_void);
type GetDeviceError = unsafe extern "C" fn(
    device: *mut std::ffi::c_void,
    out_message: *mut *const std::ffi::c_char,
) -> i32;

const OIDN_DEVICE_CPU: i32 = 1;
const OIDN_FORMAT_FLOAT3: i32 = 3;
const FILTER_TYPE: &[u8] = b"RT\0";
const NAME_COLOR: &[u8] = b"color\0";
const NAME_OUTPUT: &[u8] = b"output\0";
const NAME_SRGB: &[u8] = b"srgb\0";
const NAME_HDR: &[u8] = b"hdr\0";

pub struct Oidn {
    _lib: Library,
    previous_directory: Option<PathBuf>,
}

impl Oidn {
    pub fn load(bin_dir: &Path) -> Result<Self, String> {
        let dll = bin_dir.join("OpenImageDenoise.dll");
        if !dll.is_file() {
            return Err(format!("降噪组件不存在：{}", dll.display()));
        }
        // OIDN 2.2 延迟加载设备插件。必须在整个 OIDN 生命周期内保留 DLL
        // 目录，不能在加载主 DLL 后立刻恢复，否则 oidnNewDevice 找不到 CPU 插件。
        let old_dir = std::env::current_dir().ok();
        std::env::set_current_dir(bin_dir).map_err(|e| format!("无法切换到降噪组件目录：{e}"))?;
        let result = unsafe { Library::new("OpenImageDenoise.dll") };
        let lib = match result {
            Ok(lib) => lib,
            Err(error) => {
                if let Some(directory) = &old_dir {
                    let _ = std::env::set_current_dir(directory);
                }
                return Err(format!("无法加载降噪组件：{error}"));
            }
        };
        Ok(Self {
            _lib: lib,
            previous_directory: old_dir,
        })
    }

    pub fn denoise_gray(
        &self,
        values: &mut [f32],
        width: usize,
        height: usize,
    ) -> Result<(), String> {
        unsafe { self.denoise_gray_raw(values, width, height) }
    }

    unsafe fn denoise_gray_raw(
        &self,
        values: &mut [f32],
        width: usize,
        height: usize,
    ) -> Result<(), String> {
        let lib = &self._lib;
        let new_device: Symbol<NewDevice> = lib.get(b"oidnNewDevice\0").map_err(to_str)?;
        let commit_device: Symbol<CommitDevice> = lib.get(b"oidnCommitDevice\0").map_err(to_str)?;
        let new_filter: Symbol<NewFilter> = lib.get(b"oidnNewFilter\0").map_err(to_str)?;
        let set_shared_filter_image: Symbol<SetSharedFilterImage> =
            lib.get(b"oidnSetSharedFilterImage\0").map_err(to_str)?;
        let set_filter_bool: Symbol<SetFilterBool> =
            lib.get(b"oidnSetFilterBool\0").map_err(to_str)?;
        let commit_filter: Symbol<CommitFilter> = lib.get(b"oidnCommitFilter\0").map_err(to_str)?;
        let execute_filter: Symbol<ExecuteFilter> =
            lib.get(b"oidnExecuteFilter\0").map_err(to_str)?;
        let release_filter: Symbol<ReleaseFilter> =
            lib.get(b"oidnReleaseFilter\0").map_err(to_str)?;
        let release_device: Symbol<ReleaseDevice> =
            lib.get(b"oidnReleaseDevice\0").map_err(to_str)?;
        let get_device_error: Symbol<GetDeviceError> =
            lib.get(b"oidnGetDeviceError\0").map_err(to_str)?;

        let device = new_device(OIDN_DEVICE_CPU);
        if device.is_null() {
            return Err(oidn_error(
                &get_device_error,
                std::ptr::null_mut(),
                "降噪组件初始化失败",
            ));
        }
        commit_device(device);
        if let Some(error) = take_oidn_error(&get_device_error, device) {
            release_device(device);
            return Err(format!("降噪设备初始化失败：{error}"));
        }

        // OIDN RT filter 输入/输出为三通道浮点。AO 灰度复制到 RGB 三份。
        // 输入和输出用独立的 Rust Vec，共享给 OIDN 直接读写，无需额外拷贝。
        let pixels = width * height;
        let mut color = vec![0f32; pixels * 3];
        let mut output = vec![0f32; pixels * 3];
        for (i, value) in values.iter().enumerate() {
            let v = *value;
            color[i * 3] = v;
            color[i * 3 + 1] = v;
            color[i * 3 + 2] = v;
        }

        let filter = new_filter(device, FILTER_TYPE.as_ptr().cast());
        if filter.is_null() {
            let error = oidn_error(&get_device_error, device, "无法创建 AI 降噪滤镜");
            release_device(device);
            return Err(error);
        }
        set_shared_filter_image(
            filter,
            NAME_COLOR.as_ptr().cast(),
            color.as_mut_ptr() as *mut std::ffi::c_void,
            OIDN_FORMAT_FLOAT3,
            width,
            height,
            0,
            0,
            0,
        );
        set_shared_filter_image(
            filter,
            NAME_OUTPUT.as_ptr().cast(),
            output.as_mut_ptr() as *mut std::ffi::c_void,
            OIDN_FORMAT_FLOAT3,
            width,
            height,
            0,
            0,
            0,
        );
        set_filter_bool(filter, NAME_SRGB.as_ptr().cast(), false);
        set_filter_bool(filter, NAME_HDR.as_ptr().cast(), true);

        commit_filter(filter);
        if let Some(error) = take_oidn_error(&get_device_error, device) {
            release_filter(filter);
            release_device(device);
            return Err(format!("AI 降噪滤镜初始化失败：{error}"));
        }
        execute_filter(filter);

        if let Some(error) = take_oidn_error(&get_device_error, device) {
            release_filter(filter);
            release_device(device);
            return Err(format!("AI 降噪执行出错：{error}"));
        }

        release_filter(filter);
        release_device(device);

        // 降噪结果写回 output 数组，取 R 通道（三通道内容相同）。
        for (i, value) in values.iter_mut().enumerate() {
            *value = output[i * 3];
        }
        Ok(())
    }
}

impl Drop for Oidn {
    fn drop(&mut self) {
        if let Some(directory) = &self.previous_directory {
            let _ = std::env::set_current_dir(directory);
        }
    }
}

unsafe fn take_oidn_error(
    get_error: &GetDeviceError,
    device: *mut std::ffi::c_void,
) -> Option<String> {
    let mut message = std::ptr::null();
    let code = get_error(device, &mut message);
    if code == 0 {
        return None;
    }
    let detail = if message.is_null() {
        "未提供错误详情".to_string()
    } else {
        CStr::from_ptr(message).to_string_lossy().into_owned()
    };
    Some(format!("{detail}（OIDN 错误码 {code}）"))
}

unsafe fn oidn_error(
    get_error: &GetDeviceError,
    device: *mut std::ffi::c_void,
    fallback: &str,
) -> String {
    take_oidn_error(get_error, device).map_or_else(
        || fallback.to_string(),
        |error| format!("{fallback}：{error}"),
    )
}

fn to_str(error: libloading::Error) -> String {
    format!("降噪组件不完整：{error}")
}
