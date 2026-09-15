// 复刻宿主 spawn worker 的环境（Job Object + CREATE_NO_WINDOW + 管道），
// 用于排查 app 内导入崩溃而 CLI 直跑正常的问题。用法：
//   jobreplay <exe> <args...>
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .creation_flags(0x08000000)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows::core::PCWSTR;
        let created = unsafe { CreateJobObjectW(None, PCWSTR::null()) };
        if let Ok(job) = created {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            unsafe {
                let _ = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                let _ = AssignProcessToJobObject(job, HANDLE(child.as_raw_handle() as _));
            }
            eprintln!("[jobreplay] job attached");
        }
    }
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || {
        std::io::copy(&mut stdout, &mut std::io::stdout()).ok();
    });
    let t2 = std::thread::spawn(move || {
        std::io::copy(&mut stderr, &mut std::io::stderr()).ok();
    });
    let status = child.wait().expect("wait");
    let _ = t1.join();
    let _ = t2.join();
    eprintln!("[jobreplay] exit={:x}", status.code().unwrap_or(0));
}
