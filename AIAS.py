# GPL
# Copyright (C) 2025 AvroraCL/赫尔塔HEC&LM工作室
# Licensed under GPLv3+
# See: https://www.gnu.org/licenses/gpl-3.0.html

import os
import sys
import subprocess
import threading
import shutil
import re
import struct
import io
import json
import winreg
from functools import partial

from pathlib import Path

import requests
import numpy as np
import imageio.v2 as imageio
import qtawesome as qta
from PIL import Image, ImageDraw, ImageFilter, ImageEnhance
from PIL import ImageQt as PIL_ImageQt

from PyQt5.QtCore import Qt, pyqtSignal, QSize, QPoint, QUrl, QTimer
from PyQt5.QtGui import QPixmap, QCursor, QDesktopServices, QImage
from PyQt5.QtWidgets import (
	QApplication, QMainWindow, QWidget, QVBoxLayout, QLabel, QPushButton,
	QTextEdit, QStackedWidget, QHBoxLayout, QFileDialog, QComboBox,
	QDialog, QDialogButtonBox, QProgressBar, QCheckBox, QMessageBox,
	QGroupBox, QListWidget, QListWidgetItem, QSlider, QSpinBox, QColorDialog,
	QAbstractItemView, QFrame
)

# Config
VERSION = "4.1.0 Modern R"
COLOR_PRIMARY = "#607bc0"
COLOR_PRIMARY_HOVER = "#738bcf"
COLOR_BACKGROUND_DARKEST = "#1a1b1e"
COLOR_BACKGROUND_DARK = "#202124"
COLOR_BACKGROUND_MEDIUM = "#282a2e"
COLOR_BORDER = "#303134"
COLOR_HOVER_LIGHT = "#383a3f"
TEXT_PRIMARY = "#d3dae3"
TEXT_BRIGHT = "#ffffff"
TEXT_SECONDARY = "#8a9199"
CLOSE_BUTTON_HOVER = "#e81123"

IMAGE_EXTS = ['.png', '.tga', '.jpg', '.jpeg']
USER_DOCS = Path.home() / "Documents"
SETTINGS_DIR = USER_DOCS / "Aias"
SETTINGS_FILE = SETTINGS_DIR / "settings.json"


# Pillow Utils
def pil2pixmap(img):
	try:
		if hasattr(PIL_ImageQt, "toqimage"):
			qim = PIL_ImageQt.toqimage(img)
		else:
			ImageQtClass = getattr(PIL_ImageQt, "ImageQt")
			qim = ImageQtClass(img)
		return QPixmap.fromImage(qim)
	except Exception as e:
		print(f"Pillow to QPixmap conversion failed: {e}")
		error_img = Image.new('RGB', (100, 30), color='red')
		d = ImageDraw.Draw(error_img)
		d.text((10, 10), "Image Error", fill='white')
		return pil2pixmap(error_img)


# Utils
def find_texture_groups(folder: Path):
	files = [f for f in folder.iterdir() if f.is_file() and f.suffix.lower() in IMAGE_EXTS]
	groups = {}
	for f in files:
		name = f.stem
		lower_name = name.lower()
		for ttype in ['basecolor', 'roughness', 'metallic', 'normal']:
			if lower_name.endswith(ttype):
				prefix = name[:-len(ttype)].rstrip('_- ')
				if prefix not in groups: groups[prefix] = {}
				groups[prefix][ttype.capitalize()] = f
	return {k: v for k, v in groups.items() if all(x in v for x in ['Basecolor', 'Roughness', 'Metallic', 'Normal'])}


def calculate_dxt5_size(width, height):
	"""Calculate DXT5 byte size (16 bytes per 4x4 block)"""
	blocks_w = (width + 3) // 4
	blocks_h = (height + 3) // 4
	return blocks_w * blocks_h * 16


def extract_dds_payload(dds_bytes):
	"""Extract DDS pixel data"""
	if len(dds_bytes) < 128: raise ValueError("DDS data too short")
	if dds_bytes[:4] != b'DDS ': raise ValueError("Invalid DDS Magic")
	dwSize = struct.unpack_from('<I', dds_bytes, 4)[0]
	header_end = 4 + dwSize
	pf_flags = struct.unpack_from('<I', dds_bytes, 80)[0]
	four_cc = dds_bytes[84:88]
	if (pf_flags & 0x4) and (four_cc == b'DX10'): header_end += 20
	return dds_bytes[header_end:]


def save_dds_with_mipmaps(output_path, images, dds_format="DXT5", progress_callback=None):
	"""Save DDS with mipmaps"""
	if not images: raise ValueError("No images to save")
	rgba_images = [img.convert('RGBA') if img.mode != 'RGBA' else img for img in images]
	width, height = rgba_images[0].size
	mip_count = len(rgba_images)
	magic = b'DDS '
	dwSize = 124
	dwFlags = 0x1 | 0x2 | 0x4 | 0x1000 | 0x20000
	dwHeight = height
	dwWidth = width
	dwPitchOrLinearSize = 0
	dwDepth = 0
	dwMipMapCount = mip_count
	dwReserved1 = [0] * 11
	pf_dwSize = 32
	pf_dwFlags = 0
	pf_dwFourCC = 0
	pf_dwRGBBitCount = 0
	pf_dwRBitMask = 0
	pf_dwGBitMask = 0
	pf_dwBBitMask = 0
	pf_dwABitMask = 0

	if dds_format == "DXT5":
		pf_dwFlags = 0x4
		pf_dwFourCC = b'DXT5'
		dwPitchOrLinearSize = calculate_dxt5_size(width, height)
		dwFlags |= 0x80000
	else:
		pf_dwFlags = 0x40 | 0x1
		pf_dwRGBBitCount = 32
		pf_dwRBitMask = 0x00ff0000
		pf_dwGBitMask = 0x0000ff00
		pf_dwBBitMask = 0x000000ff
		pf_dwABitMask = 0xff000000
		dwPitchOrLinearSize = width * 4
		dwFlags |= 0x8

	caps_dwCaps1 = 0x1000 | 0x400000 | 0x8
	caps_dwCaps2 = 0
	caps_dwCaps3 = 0
	caps_dwCaps4 = 0
	dwReserved2 = 0

	header = struct.pack('<I IIIIII 11I 2I4s5I 4I I',
	                     dwSize, dwFlags, dwHeight, dwWidth, dwPitchOrLinearSize, dwDepth, dwMipMapCount, *dwReserved1,
	                     pf_dwSize, pf_dwFlags, pf_dwFourCC if isinstance(pf_dwFourCC, bytes) else b'\0\0\0\0',
	                     pf_dwRGBBitCount, pf_dwRBitMask, pf_dwGBitMask, pf_dwBBitMask, pf_dwABitMask,
	                     caps_dwCaps1, caps_dwCaps2, caps_dwCaps3, caps_dwCaps4, dwReserved2)

	with open(output_path, 'wb') as f:
		f.write(magic)
		f.write(header)
		for i, img in enumerate(rgba_images):
			if progress_callback:
				progress = int((i + 1) / len(rgba_images) * 100)
				progress_callback(progress)
			if dds_format == "DXT5":
				with io.BytesIO() as buf:
					imageio.imwrite(buf, img, format='DDS', flags='DXT5')
					buf.seek(0)
					dds_data = buf.read()
					pixel_data = extract_dds_payload(dds_data)
					f.write(pixel_data)
			else:
				r, g, b, a = img.split()
				bgra = Image.merge("RGBA", (b, g, r, a))
				f.write(bgra.tobytes())


def save_single_dds(output_path, image, dds_format="DXT5", alpha_color=None):
	"""Save single image to DDS"""
	if image.mode != 'RGBA': image = image.convert('RGBA')
	if alpha_color == 'black':
		image.putalpha(Image.new('L', image.size, 0))
	elif alpha_color == 'white':
		image.putalpha(Image.new('L', image.size, 255))
	imageio.imwrite(str(output_path), image, format='DDS', flags=dds_format)


# UI Widgets
class DropTargetWidget(QWidget):
	filesDropped = pyqtSignal(list)

	def __init__(self, parent=None):
		super().__init__(parent)
		self.setAcceptDrops(True)
		self.setObjectName("DropTarget")

	def dragEnterEvent(self, event):
		if event.mimeData().hasUrls():
			event.acceptProposedAction()
			self.setProperty("drop-active", True)
			self.style().unpolish(self); self.style().polish(self)
		else:
			event.ignore()

	def dragLeaveEvent(self, event):
		self.setProperty("drop-active", False)
		self.style().unpolish(self); self.style().polish(self)

	def dropEvent(self, event):
		self.setProperty("drop-active", False)
		self.style().unpolish(self); self.style().polish(self)
		urls = event.mimeData().urls()
		if urls: self.filesDropped.emit([url.toLocalFile() for url in urls])


# Skin Manager Custom Widgets
class SkinListItem(QWidget):
	"""Custom widget for a single skin file in the list"""
	deleteRequested = pyqtSignal()
	openRequested = pyqtSignal()
	toggleRequested = pyqtSignal()

	def __init__(self, file_path, is_disabled, parent=None):
		super().__init__(parent)
		self.file_path = Path(file_path)
		self.is_disabled = is_disabled
		self.setFixedHeight(45)
		self.setObjectName("SkinListItem")

		layout = QHBoxLayout(self)
		layout.setContentsMargins(10, 0, 10, 0)
		layout.setSpacing(10)

		# Filename Label
		self.name_label = QLabel(self.file_path.name)
		self.name_label.setStyleSheet(f"color: {TEXT_SECONDARY if is_disabled else TEXT_PRIMARY}; font-size: 10pt;")
		layout.addWidget(self.name_label, 1)

		# Toggle Button
		self.btn_toggle = QPushButton()
		self.btn_toggle.setFixedSize(40, 30)
		self.btn_toggle.setCursor(Qt.PointingHandCursor)
		self.btn_toggle.setObjectName("SkinToggleBtn")
		self.update_toggle_icon()
		self.btn_toggle.clicked.connect(self.toggleRequested.emit)
		layout.addWidget(self.btn_toggle)

		# Open Folder Button
		self.btn_open = QPushButton(qta.icon('fa5s.folder-open', color=TEXT_PRIMARY), "")
		self.btn_open.setFixedSize(30, 30)
		self.btn_open.setCursor(Qt.PointingHandCursor)
		self.btn_open.setObjectName("SkinActionBtn")
		self.btn_open.clicked.connect(self.openRequested.emit)
		layout.addWidget(self.btn_open)

		# Delete Button
		self.btn_delete = QPushButton(qta.icon('fa5s.trash', color=TEXT_PRIMARY), "")
		self.btn_delete.setFixedSize(30, 30)
		self.btn_delete.setCursor(Qt.PointingHandCursor)
		self.btn_delete.setObjectName("SkinActionBtn")
		self.btn_delete.clicked.connect(self.deleteRequested.emit)
		layout.addWidget(self.btn_delete)

	def update_toggle_icon(self):
		if self.is_disabled:
			self.btn_toggle.setIcon(qta.icon('fa5s.toggle-off', color=TEXT_SECONDARY))
			self.btn_toggle.setToolTip("启用涂装")
		else:
			self.btn_toggle.setIcon(qta.icon('fa5s.toggle-on', color=COLOR_PRIMARY))
			self.btn_toggle.setToolTip("禁用涂装")


class SkinListWidget(QListWidget):
	"""List widget that accepts file drops"""
	filesDropped = pyqtSignal(list)

	def __init__(self, parent=None):
		super().__init__(parent)
		self.setAcceptDrops(True)
		self.setDragDropMode(QAbstractItemView.DropOnly)
		self.setSelectionMode(QAbstractItemView.NoSelection)
		self.setSpacing(2)

	def dragEnterEvent(self, event):
		if event.mimeData().hasUrls():
			event.acceptProposedAction()
		else:
			event.ignore()

	def dragMoveEvent(self, event):
		if event.mimeData().hasUrls():
			event.acceptProposedAction()
		else:
			event.ignore()

	def dropEvent(self, event):
		urls = event.mimeData().urls()
		if urls:
			files = [url.toLocalFile() for url in urls]
			self.filesDropped.emit(files)


# Dialog Base
class CustomDialog(QDialog):
	def __init__(self, parent=None, title="Dialog"):
		super().__init__(parent)
		self.setWindowFlags(Qt.FramelessWindowHint | Qt.Dialog)
		self.setAttribute(Qt.WA_TranslucentBackground)
		self.setObjectName("CustomDialog")
		self.setModal(True)
		self.old_pos = None
		self._title = title
		container = QWidget(self)
		container.setObjectName("DialogContainer")
		self.main_layout = QVBoxLayout(self)
		self.main_layout.setContentsMargins(0, 0, 0, 0)
		self.main_layout.addWidget(container)
		dialog_layout = QVBoxLayout(container)
		dialog_layout.setContentsMargins(1, 1, 1, 1)
		dialog_layout.setSpacing(0)
		self.title_bar = self._create_title_bar()
		dialog_layout.addWidget(self.title_bar)
		self.content_widget = QWidget()
		self.content_layout = QVBoxLayout(self.content_widget)
		self.content_layout.setContentsMargins(15, 15, 15, 15)
		dialog_layout.addWidget(self.content_widget, 1)

	def _create_title_bar(self):
		title_bar = QWidget()
		title_bar.setObjectName("DialogTitleBar")
		title_bar.setFixedHeight(35)
		layout = QHBoxLayout(title_bar)
		layout.setContentsMargins(10, 0, 5, 0)
		title_label = QLabel(self._title)
		title_label.setObjectName("TitleLabel")
		btn_close = QPushButton(qta.icon('fa5s.times', color=TEXT_PRIMARY), "")
		btn_close.setObjectName("CloseButton")
		btn_close.setFixedSize(25, 25)
		btn_close.clicked.connect(self.reject)
		layout.addWidget(title_label)
		layout.addStretch()
		layout.addWidget(btn_close)
		return title_bar

	def center_on_parent(self):
		if self.parent():
			parent_rect = self.parent().geometry()
			self_rect = self.geometry()
			x = parent_rect.x() + (parent_rect.width() - self_rect.width()) // 2
			y = parent_rect.y() + (parent_rect.height() - self_rect.height()) // 2
			self.move(x, y)

	def exec_(self):
		if self.parent(): self.center_on_parent()
		return super().exec_()

	def mousePressEvent(self, event):
		if event.button() == Qt.LeftButton and event.y() < self.title_bar.height(): self.old_pos = event.globalPos()

	def mouseMoveEvent(self, event):
		if event.buttons() == Qt.LeftButton and self.old_pos:
			delta = QPoint(event.globalPos() - self.old_pos)
			self.move(self.x() + delta.x(), self.y() + delta.y())
			self.old_pos = event.globalPos()

	def mouseReleaseEvent(self, event):
		self.old_pos = None


# Confirm Dialog
class ConfirmDialog(CustomDialog):
	def __init__(self, parent=None, title="确认", message=""):
		super().__init__(parent, title=title)
		layout = self.content_layout

		msg_label = QLabel(message)
		msg_label.setWordWrap(True)
		msg_label.setAlignment(Qt.AlignCenter)
		msg_label.setStyleSheet("font-size: 11pt; padding: 20px;")
		layout.addWidget(msg_label)

		btn_box = QDialogButtonBox(QDialogButtonBox.Yes | QDialogButtonBox.No)
		btn_yes = btn_box.button(QDialogButtonBox.Yes)
		btn_no = btn_box.button(QDialogButtonBox.No)
		btn_yes.setText("确认删除")
		btn_no.setText("取消")

		# Custom styling for the delete button (Red)
		btn_yes.setStyleSheet(f"""
			QPushButton {{
				background-color: {CLOSE_BUTTON_HOVER}; 
				color: white; 
				font-weight: bold; 
				padding: 8px 20px; 
				border-radius: 4px;
			}}
			QPushButton:hover {{ background-color: #ec4a58; }}
		""")

		btn_box.accepted.connect(self.accept)
		btn_box.rejected.connect(self.reject)
		layout.addWidget(btn_box)


# Settings Dialog
class SettingsDialog(CustomDialog):
	def __init__(self, parent=None, settings=None):
		super().__init__(parent, title="设置")
		self.setMinimumWidth(450)
		self.settings = settings or {}
		self.main_window = parent
		layout = self.content_layout
		layout.setSpacing(15)
		update_group = QGroupBox("软件更新")
		update_layout = QVBoxLayout(update_group)
		self.btn_check_update = QPushButton(qta.icon('fa5s.cloud-download-alt', color=TEXT_PRIMARY), " 检查更新")
		self.btn_install_update = QPushButton(qta.icon('fa5s.sync-alt', color=TEXT_PRIMARY), " 安装更新")
		self.btn_install_update.setEnabled(False)
		self.chk_auto_update = QCheckBox("启动时自动检查更新")
		self.chk_auto_update.setChecked(self.settings.get("auto_update", True))
		update_layout.addWidget(self.btn_check_update)
		update_layout.addWidget(self.btn_install_update)
		update_layout.addWidget(self.chk_auto_update)
		layout.addWidget(update_group)
		cache_group = QGroupBox("软件缓存")
		cache_layout = QVBoxLayout(cache_group)
		self.btn_clear_cache = QPushButton(qta.icon('fa5s.trash-alt', color=TEXT_PRIMARY), " 清理软件缓存")
		cache_layout.addWidget(self.btn_clear_cache)
		layout.addWidget(cache_group)
		self.progress_bar = QProgressBar()
		self.progress_bar.setVisible(False)
		layout.addWidget(self.progress_bar)
		self.btn_check_update.clicked.connect(self.check_update)
		self.btn_install_update.clicked.connect(self.install_update)
		self.chk_auto_update.stateChanged.connect(self.toggle_auto_update)
		self.btn_clear_cache.clicked.connect(self.clear_cache)
		self.update_available = False
		self.latest_version = None
		self.update_url = None
		self.update_path = None

	def toggle_auto_update(self, state):
		self.settings["auto_update"] = (state == Qt.Checked)
		self.save_settings()

	def save_settings(self):
		try:
			SETTINGS_DIR.mkdir(exist_ok=True)
			with open(SETTINGS_FILE, "w", encoding="utf-8") as f:
				json.dump(self.settings, f, indent=2)
		except Exception as e:
			QMessageBox.warning(self, "错误", f"保存设置失败: {e}")

	def clear_cache(self):
		cache_dir = SETTINGS_DIR / "cache"
		if cache_dir.exists() and cache_dir.is_dir():
			try:
				shutil.rmtree(cache_dir)
				QMessageBox.information(self, "清理缓存", "缓存已成功清理。")
			except Exception as e:
				QMessageBox.warning(self, "错误", f"清理缓存失败: {e}")
		else:
			QMessageBox.information(self, "清理缓存", "无缓存需要清理。")

	def check_update(self):
		self.btn_check_update.setEnabled(False)
		self.progress_bar.setVisible(True)
		self.progress_bar.setRange(0, 0)

		def do_check():
			urls = ["https://gitcode.com/HelenaSG/Aias/raw/master/version.json",
			        "https://api.github.com/repos/AvroraCL/Aias/releases/latest"]
			version, download_url = None, None
			for url in urls:
				try:
					r = requests.get(url, timeout=5)
					r.raise_for_status()
					data = r.json()
					if "gitcode" in url:
						version, download_url = data.get("version"), data.get("download_url")
					elif "github" in url:
						version = data.get("tag_name", "").lstrip('v')
						assets = data.get("assets", [])
						download_url = next(
							(a.get("browser_download_url") for a in assets if a.get("name", "").endswith(".exe")), None)
					if version and download_url: break
				except requests.RequestException:
					continue
			self.latest_version, self.update_url = version, download_url
			self.main_window.invoke_in_main_thread(self.finish_check)

		threading.Thread(target=do_check, daemon=True).start()

	def finish_check(self):
		self.progress_bar.setVisible(False)
		self.btn_check_update.setEnabled(True)
		if self.latest_version and self.compare_versions(self.latest_version, VERSION.split()[0]) > 0:
			self.update_available = True
			self.btn_install_update.setEnabled(True)
			QMessageBox.information(self, "更新可用", f"检测到新版本 {self.latest_version}，请点击\"安装更新\"。")
		else:
			self.update_available = False
			self.btn_install_update.setEnabled(False)
			QMessageBox.information(self, "无更新", "当前已是最新版本。")

	def install_update(self):
		if not self.update_available or not self.update_url:
			QMessageBox.information(self, "提示", "没有可用的更新。")
			return
		self.btn_install_update.setEnabled(False)
		self.progress_bar.setVisible(True)
		self.progress_bar.setRange(0, 100)
		self.progress_bar.setValue(0)

		def do_install():
			try:
				self.update_path = SETTINGS_DIR / "Aias_update_tmp.exe"
				with requests.get(self.update_url, stream=True, timeout=15) as r:
					r.raise_for_status()
					total = int(r.headers.get('content-length', 0))
					downloaded = 0
					with open(self.update_path, 'wb') as f:
						for chunk in r.iter_content(chunk_size=8192):
							f.write(chunk)
							downloaded += len(chunk)
							percent = int(downloaded * 100 / total) if total else 0
							self.main_window.invoke_in_main_thread(lambda p=percent: self.progress_bar.setValue(p))
				self.main_window.invoke_in_main_thread(self.prompt_for_install)
			except Exception as e:
				self.main_window.invoke_in_main_thread(
					lambda: QMessageBox.warning(self, "更新失败", f"下载更新失败: {e}"))
			finally:
				self.main_window.invoke_in_main_thread(self.finish_install)

		threading.Thread(target=do_install, daemon=True).start()

	def prompt_for_install(self):
		reply = QMessageBox.question(self, "下载完成",
		                             f"更新已下载完毕。\n是否立即启动安装程序？\n\n注意：本程序将会关闭。",
		                             QMessageBox.Yes | QMessageBox.No, QMessageBox.Yes)
		if reply == QMessageBox.Yes and self.update_path:
			try:
				subprocess.Popen([str(self.update_path)])
				QApplication.instance().quit()
			except Exception as e:
				QMessageBox.warning(self, "启动失败",
				                    f"无法自动启动安装程序: {e}\n请手动前往以下路径运行：\n{self.update_path}")

	def finish_install(self):
		self.progress_bar.setVisible(False)
		self.btn_install_update.setEnabled(True)

	@staticmethod
	def compare_versions(v1, v2):
		p1 = [int(x) for x in re.findall(r'\d+', v1)]
		p2 = [int(x) for x in re.findall(r'\d+', v2)]
		return (p1 > p2) - (p1 < p2)


# Help Dialog
class HelpDialog(CustomDialog):
	def __init__(self, parent=None):
		super().__init__(parent, title="帮助信息")
		self.resize(800, 600)
		layout = self.content_layout
		self.help_text = QTextEdit()
		self.help_text.setReadOnly(True)
		help_content = f"""
        <style> h1, h2, h3 {{ color: {COLOR_PRIMARY}; }} ul {{ padding-left: 20px; }} </style>
        <h1>欢迎使用 AIAS 工具箱 v{VERSION.split()[0]}</h1>
        <p>本程序由 <b>AvroraCL</b> 开发，基于 GPLv3 协议开源。</p>
        <h2>注意事项</h2>
        <ul><li>v{VERSION.split()[0]} 版本主要支持 Adobe Substance 3D Painter 导出的 Blender BSDF 格式贴图。</li><li>请确保电脑有足够的运行内存和存储空间以保证程序稳定运行。</li><li>PBR和Mipmap功能已移除外部依赖，现使用纯Python实现。</li></ul>
        <h2>功能说明</h2>
        <h3>PBR多通道合成</h3><p>自动扫描并合并PBR贴图组 (BaseColor, Roughness, Metallic, Normal) 为游戏引擎优化格式。</p>
        <h3>PBR多通道拆分</h3><p>自动扫描并拆分包含PBR信息的DDS文件 (_c.dds, _n.dds) 为独立的PNG或TGA贴图文件。<b>支持拖放文件或文件夹到窗口内。</b></p>
        <h3>Mipmap生成</h3><p>将 p0.png 转换为 DDS 格式。注意：此功能现使用 Python 内置库，不再依赖 texassemble。</p>
        <h3>图片转DDS</h3><p>将PNG、TGA、JPG等图片转换为DDS格式，可选择黑白色通道和8.8.8.8以及DXT5压缩方式。</p>
        <h3>涂装管理器</h3><p>自动检索War Thunder游戏目录下的UserSkins文件夹，并管理其中的涂装文件。支持拖拽导入、启用/禁用、删除等操作。</p><hr>
        <p><b>作者链接 (请复制到浏览器中打开):</b></p>
        <ul>
            <li>WT Live: https://live.warthunder.com/user/107729661</li>
            <li>Bilibili: https://space.bilibili.com/38636319</li>
            <li>Github: https://github.com/AvroraCL</li>
        </ul>"""
		self.help_text.setHtml(help_content)
		layout.addWidget(self.help_text)
		buttons = QDialogButtonBox(QDialogButtonBox.Ok)
		buttons.accepted.connect(self.accept)
		layout.addWidget(buttons)


# Main Window
class AIASMainWindow(QMainWindow):
	invoke_signal = pyqtSignal(object)

	def __init__(self):
		super().__init__()
		self.setWindowTitle(f"AIAS v{VERSION}")
		self.setWindowFlags(Qt.FramelessWindowHint)
		self.setAttribute(Qt.WA_TranslucentBackground)
		self.resize(1200, 800)
		self.setWindowIcon(qta.icon('fa5s.rocket', color='white'))
		self.invoke_signal.connect(self._invoke)
		self.settings = self.load_settings()
		self.init_ui()
		if self.settings.get("auto_update", True): self.check_update_in_background()
		self.old_pos = self.pos()

	def init_ui(self):
		self.container = QWidget()
		self.container.setObjectName("MainContainer")
		self.setCentralWidget(self.container)
		main_layout = QVBoxLayout(self.container)
		main_layout.setContentsMargins(0, 0, 0, 0)
		main_layout.setSpacing(0)
		self.title_bar = self._create_title_bar()
		main_layout.addWidget(self.title_bar)
		content_layout = QHBoxLayout()
		content_layout.setContentsMargins(0, 0, 0, 0)
		content_layout.setSpacing(0)
		self.nav_bar = QListWidget()
		self.nav_bar.setObjectName("NavBar")
		self.nav_bar.setFixedWidth(200)
		content_layout.addWidget(self.nav_bar)
		self.stack = QStackedWidget()
		content_layout.addWidget(self.stack)
		main_layout.addLayout(content_layout)
		self.pbr_widget = self._create_texture_processing_ui("pbr", "PBR多通道合成", {"alpha": (["纯黑", "纯白"]),
		                                                                              "dds": (["DTX5", "8.8.8.8"])})
		self.pbr_split_widget = self._create_pbr_split_ui()
		self.mipmap_widget = self._create_texture_processing_ui("mipmap", "Mipmap生成", {"alpha": (["纯黑", "纯白"]),
		                                                                                 "dds": (["DTX5", "8.8.8.8"])})
		self.image_to_dds_widget = self._create_image_to_dds_ui()
		self.skin_manager_widget = self._create_skin_manager_ui()
		self._add_nav_item('fa5s.cubes', "PBR多通道合成", self.pbr_widget)
		self._add_nav_item('fa5s.object-ungroup', "PBR多通道拆分", self.pbr_split_widget)
		self._add_nav_item('fa5s.layer-group', "Mipmap生成", self.mipmap_widget)
		self._add_nav_item('fa5s.image', "图片转DDS", self.image_to_dds_widget)
		self._add_nav_item('fa5s.tshirt', "涂装管理器", self.skin_manager_widget)
		self.nav_bar.currentRowChanged.connect(self.stack.setCurrentIndex)
		self.nav_bar.setCurrentRow(0)
		self.status_bar = self.statusBar()
		self.status_bar.setObjectName("StatusBar")
		status_buttons_widget = QWidget()
		status_buttons_layout = QHBoxLayout(status_buttons_widget)
		status_buttons_layout.setContentsMargins(5, 0, 5, 0)
		status_buttons_layout.setSpacing(5)
		btn_settings = QPushButton(qta.icon('fa5s.cog', color=TEXT_PRIMARY), "")
		btn_settings.setObjectName("StatusBarButton");
		btn_settings.setToolTip("设置")
		btn_settings.clicked.connect(self.show_settings_dialog)
		btn_help = QPushButton(qta.icon('fa5s.question-circle', color=TEXT_PRIMARY), "")
		btn_help.setObjectName("StatusBarButton");
		btn_help.setToolTip("帮助")
		btn_help.clicked.connect(self.show_help_dialog)
		status_buttons_layout.addWidget(btn_settings);
		status_buttons_layout.addWidget(btn_help)
		self.status_bar.addWidget(status_buttons_widget)
		self.version_label = QLabel(f"v{VERSION.split()[0]}")
		self.status_bar.addPermanentWidget(self.version_label)

	def _create_title_bar(self):
		title_bar = QWidget()
		title_bar.setObjectName("TitleBar")
		title_bar.setFixedHeight(40)
		layout = QHBoxLayout(title_bar)
		layout.setContentsMargins(10, 0, 0, 0)
		layout.setSpacing(10)
		title_label = QLabel(f"AIAS v{VERSION.split()[0]}")
		title_label.setObjectName("TitleLabel")
		layout.addWidget(title_label);
		layout.addStretch()
		btn_minimize = QPushButton(qta.icon('fa5s.window-minimize', color=TEXT_PRIMARY), "")
		btn_maximize = QPushButton(qta.icon('fa5s.window-maximize', color=TEXT_PRIMARY), "")
		btn_close = QPushButton(qta.icon('fa5s.times', color=TEXT_PRIMARY), "")
		btn_minimize.setObjectName("TitleBarButton");
		btn_maximize.setObjectName("TitleBarButton");
		btn_close.setObjectName("CloseButton")
		btn_minimize.setFixedSize(30, 30);
		btn_maximize.setFixedSize(30, 30);
		btn_close.setFixedSize(30, 30)
		btn_minimize.clicked.connect(self.showMinimized)
		btn_maximize.clicked.connect(self.toggle_maximize)
		btn_close.clicked.connect(self.close)
		layout.addWidget(btn_minimize);
		layout.addWidget(btn_maximize);
		layout.addWidget(btn_close)
		return title_bar

	def toggle_maximize(self):
		if self.isMaximized(): self.showNormal()
		else: self.showMaximized()

	def mousePressEvent(self, event):
		if event.button() == Qt.LeftButton and event.y() < self.title_bar.height(): self.old_pos = event.globalPos()

	def mouseMoveEvent(self, event):
		if event.buttons() == Qt.LeftButton and self.old_pos:
			delta = QPoint(event.globalPos() - self.old_pos)
			self.move(self.x() + delta.x(), self.y() + delta.y())
			self.old_pos = event.globalPos()

	def mouseReleaseEvent(self, event):
		self.old_pos = None

	def _add_nav_item(self, icon_name, text, widget):
		item = QListWidgetItem(qta.icon(icon_name, color=TEXT_PRIMARY, color_active=TEXT_BRIGHT), text)
		item.setSizeHint(QSize(40, 45))
		self.nav_bar.addItem(item)
		if widget: self.stack.addWidget(widget)

	def _create_texture_processing_ui(self, name, title, options):
		widget = QWidget()
		layout = QVBoxLayout(widget)
		layout.setContentsMargins(20, 20, 20, 20);
		layout.setSpacing(15)
		folder_group = QGroupBox("文件路径")
		folder_layout = QVBoxLayout(folder_group)
		input_path = self.settings.get(f"{name}_input_path", "")
		output_path = self.settings.get(f"{name}_output_path", "")
		input_label = QLabel(f"输入文件夹: {input_path or '未选择'}")
		output_label = QLabel(f"输出文件夹: {output_path or '未选择'}")
		btn_input = QPushButton(qta.icon('fa5s.folder-open', color=TEXT_PRIMARY), " 选择输入文件夹")
		btn_output = QPushButton(qta.icon('fa5s.folder', color=TEXT_PRIMARY), " 选择输出文件夹")
		btn_input.clicked.connect(lambda: self.select_folder(name, 'input', input_label))
		btn_output.clicked.connect(lambda: self.select_folder(name, 'output', output_label))
		folder_layout.addWidget(input_label);
		folder_layout.addWidget(btn_input)
		folder_layout.addWidget(output_label);
		folder_layout.addWidget(btn_output)
		layout.addWidget(folder_group)
		options_group = QGroupBox("导出选项")
		options_layout = QHBoxLayout(options_group)
		alpha_combo = QComboBox()
		alpha_combo.addItems(options["alpha"])
		alpha_combo.setCurrentText(self.settings.get(f"{name}_alpha_choice", options["alpha"][0]))
		dds_combo = QComboBox()
		dds_combo.addItems(options["dds"])
		dds_combo.setCurrentText(self.settings.get(f"{name}_dds_format_choice", options["dds"][0]))
		options_layout.addWidget(QLabel("Alpha通道:"));
		options_layout.addWidget(alpha_combo)
		options_layout.addSpacing(20)
		options_layout.addWidget(QLabel("DDS格式:"));
		options_layout.addWidget(dds_combo)
		options_layout.addStretch(1)
		layout.addWidget(options_group)
		log_group = QGroupBox("日志")
		log_layout = QVBoxLayout(log_group)
		log_layout.setContentsMargins(5, 10, 5, 5)
		log_edit = QTextEdit()
		log_edit.setReadOnly(True)
		progress_bar = QProgressBar()
		progress_bar.setTextVisible(False)
		log_layout.addWidget(log_edit);
		log_layout.addWidget(progress_bar)
		layout.addWidget(log_group, 1)
		btn_run = QPushButton(qta.icon('fa5s.play-circle', color=TEXT_BRIGHT), f" 开始{title}")
		btn_run.setObjectName("RunButton")
		layout.addWidget(btn_run)
		setattr(self, f"{name}_widgets",
		        {"input_label": input_label, "output_label": output_label, "alpha_combo": alpha_combo,
		         "dds_combo": dds_combo, "log": log_edit, "progress": progress_bar, "run_button": btn_run})
		if name == "pbr":
			btn_run.clicked.connect(self.run_merge)
		elif name == "mipmap":
			btn_run.clicked.connect(self.run_mipmap)
		return widget

	def _create_pbr_split_ui(self):
		name, title = "pbr_split", "PBR多通道拆分"
		self.pbr_split_files = set()
		widget = DropTargetWidget()
		widget.filesDropped.connect(self.handle_pbr_split_drop)
		layout = QVBoxLayout(widget)
		layout.setContentsMargins(20, 20, 20, 20);
		layout.setSpacing(15)
		input_group = QGroupBox("输入文件 (可拖放文件至此窗口)")
		input_layout = QVBoxLayout(input_group)
		self.pbr_split_file_list = QListWidget()
		self.pbr_split_file_list.setFixedHeight(150)
		input_btn_layout = QHBoxLayout()
		btn_select_files = QPushButton(qta.icon('fa5s.file-import', color=TEXT_PRIMARY), " 选择DDS文件")
		btn_select_files.clicked.connect(self.select_dds_files)
		btn_clear_list = QPushButton(qta.icon('fa5s.trash', color=TEXT_PRIMARY), " 清空列表")
		btn_clear_list.clicked.connect(self.clear_pbr_split_list)
		input_btn_layout.addWidget(btn_select_files)
		input_btn_layout.addWidget(btn_clear_list)
		input_layout.addWidget(self.pbr_split_file_list)
		input_layout.addLayout(input_btn_layout)
		layout.addWidget(input_group)
		output_group = QGroupBox("输出设置")
		output_layout = QVBoxLayout(output_group)
		output_path = self.settings.get(f"{name}_output_path", "")
		output_label = QLabel(f"输出文件夹 (保存图片): {output_path or '未选择'}")
		btn_output = QPushButton(qta.icon('fa5s.folder', color=TEXT_PRIMARY), " 选择输出文件夹")
		btn_output.clicked.connect(lambda: self.select_folder(name, 'output', output_label))
		output_layout.addWidget(output_label)
		output_layout.addWidget(btn_output)
		layout.addWidget(output_group)
		options_group = QGroupBox("导出选项")
		options_layout = QHBoxLayout(options_group)
		format_combo = QComboBox()
		format_combo.addItems(["PNG", "TGA"])
		format_combo.setCurrentText(self.settings.get(f"{name}_export_format", "TGA"))
		chk_export_alpha = QCheckBox("从C文件中导出Alpha通道")
		chk_export_alpha.setChecked(self.settings.get(f"{name}_export_alpha", True))
		options_layout.addWidget(QLabel("导出格式:"))
		options_layout.addWidget(format_combo)
		options_layout.addSpacing(20)
		options_layout.addWidget(chk_export_alpha)
		options_layout.addStretch(1)
		layout.addWidget(options_group)
		log_group = QGroupBox("日志")
		log_layout = QVBoxLayout(log_group)
		log_edit = QTextEdit();
		log_edit.setReadOnly(True)
		progress_bar = QProgressBar();
		progress_bar.setTextVisible(False)
		log_layout.addWidget(log_edit);
		log_layout.addWidget(progress_bar)
		layout.addWidget(log_group, 1)
		btn_run = QPushButton(qta.icon('fa5s.play-circle', color=TEXT_BRIGHT), f" 开始{title}")
		btn_run.setObjectName("RunButton")
		btn_run.clicked.connect(self.run_split)
		layout.addWidget(btn_run)
		setattr(self, f"{name}_widgets",
		        {"output_label": output_label, "export_alpha_chk": chk_export_alpha, "format_combo": format_combo,
		         "log": log_edit, "progress": progress_bar, "run_button": btn_run})
		return widget

	def _create_image_to_dds_ui(self):
		name, title = "image_to_dds", "图片转DDS"
		self.image_to_dds_files = set()
		widget = DropTargetWidget()
		widget.filesDropped.connect(self.handle_image_to_dds_drop)
		layout = QVBoxLayout(widget)
		layout.setContentsMargins(20, 20, 20, 20);
		layout.setSpacing(15)
		input_group = QGroupBox("输入文件 (可拖放文件至此窗口)")
		input_layout = QVBoxLayout(input_group)
		self.image_to_dds_file_list = QListWidget()
		self.image_to_dds_file_list.setFixedHeight(150)
		input_btn_layout = QHBoxLayout()
		btn_select_files = QPushButton(qta.icon('fa5s.file-import', color=TEXT_PRIMARY), " 选择图片文件")
		btn_select_files.clicked.connect(self.select_image_files)
		btn_clear_list = QPushButton(qta.icon('fa5s.trash', color=TEXT_PRIMARY), " 清空列表")
		btn_clear_list.clicked.connect(self.clear_image_to_dds_list)
		input_btn_layout.addWidget(btn_select_files)
		input_btn_layout.addWidget(btn_clear_list)
		input_layout.addWidget(self.image_to_dds_file_list)
		input_layout.addLayout(input_btn_layout)
		layout.addWidget(input_group)
		output_group = QGroupBox("输出设置")
		output_layout = QVBoxLayout(output_group)
		output_path = self.settings.get(f"{name}_output_path", "")
		output_label = QLabel(f"输出文件夹 (保存DDS): {output_path or '未选择'}")
		btn_output = QPushButton(qta.icon('fa5s.folder', color=TEXT_PRIMARY), " 选择输出文件夹")
		btn_output.clicked.connect(lambda: self.select_folder(name, 'output', output_label))
		output_layout.addWidget(output_label)
		output_layout.addWidget(btn_output)
		layout.addWidget(output_group)
		options_group = QGroupBox("导出选项")
		options_layout = QHBoxLayout(options_group)
		alpha_combo = QComboBox()
		alpha_combo.addItems(["纯黑", "纯白", "保留原图"])
		alpha_combo.setCurrentText(self.settings.get(f"{name}_alpha_choice", "保留原图"))
		dds_combo = QComboBox()
		dds_combo.addItems(["DTX5", "8.8.8.8"])
		dds_combo.setCurrentText(self.settings.get(f"{name}_dds_format_choice", "DTX5"))
		options_layout.addWidget(QLabel("Alpha通道:"))
		options_layout.addWidget(alpha_combo)
		options_layout.addSpacing(20)
		options_layout.addWidget(QLabel("DDS格式:"))
		options_layout.addWidget(dds_combo)
		options_layout.addStretch(1)
		layout.addWidget(options_group)
		log_group = QGroupBox("日志")
		log_layout = QVBoxLayout(log_group)
		log_edit = QTextEdit();
		log_edit.setReadOnly(True)
		progress_bar = QProgressBar();
		progress_bar.setTextVisible(False)
		log_layout.addWidget(log_edit);
		log_layout.addWidget(progress_bar)
		layout.addWidget(log_group, 1)
		btn_run = QPushButton(qta.icon('fa5s.play-circle', color=TEXT_BRIGHT), f" 开始{title}")
		btn_run.setObjectName("RunButton")
		btn_run.clicked.connect(self.run_image_to_dds)
		layout.addWidget(btn_run)
		setattr(self, f"{name}_widgets",
		        {"output_label": output_label, "alpha_combo": alpha_combo, "dds_combo": dds_combo,
		         "log": log_edit, "progress": progress_bar, "run_button": btn_run})
		return widget

	def _create_skin_manager_ui(self):
		name = "skin_manager"
		widget = QWidget()
		main_layout = QHBoxLayout(widget)
		main_layout.setContentsMargins(0, 0, 0, 0)
		main_layout.setSpacing(0)

		# Left: A-Z Search Bar
		az_bar = QWidget()
		az_bar.setObjectName("AZBar")
		az_bar.setFixedWidth(40)
		az_layout = QVBoxLayout(az_bar)
		az_layout.setContentsMargins(0, 5, 0, 5)
		az_layout.setSpacing(2)

		letters = ['#'] + [chr(i) for i in range(ord('A'), ord('Z') + 1)]
		for char in letters:
			btn = QPushButton(char)
			btn.setFixedSize(30, 20)
			btn.setObjectName("AZButton")
			btn.clicked.connect(lambda checked, c=char: self.filter_skins(c))
			az_layout.addWidget(btn)
		az_layout.addStretch()
		main_layout.addWidget(az_bar)

		# Right: Content
		content_layout = QVBoxLayout()
		content_layout.setContentsMargins(20, 20, 20, 20)
		content_layout.setSpacing(15)

		# Path Group
		path_group = QGroupBox("涂装目录")
		path_layout = QHBoxLayout(path_group)
		self.skin_path_label = QLabel("正在自动检测...")
		self.skin_path_label.setWordWrap(True)
		self.skin_path_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
		btn_refresh = QPushButton(qta.icon('fa5s.sync-alt', color=TEXT_PRIMARY), " 刷新")
		btn_manual = QPushButton(qta.icon('fa5s.folder-open', color=TEXT_PRIMARY), " 手动选择")
		path_layout.addWidget(self.skin_path_label)
		path_layout.addWidget(btn_refresh)
		path_layout.addWidget(btn_manual)
		content_layout.addWidget(path_group)

		# File List
		list_group = QGroupBox("涂装文件列表 (支持拖拽导入)")
		list_layout = QVBoxLayout(list_group)
		self.skin_list_widget = SkinListWidget()
		self.skin_list_widget.filesDropped.connect(self.handle_skin_drop)
		list_layout.addWidget(self.skin_list_widget)
		content_layout.addWidget(list_group)

		main_layout.addLayout(content_layout)

		# Connections
		btn_refresh.clicked.connect(self.refresh_skin_list)
		btn_manual.clicked.connect(self.select_skin_folder)

		# Initial Load
		self.skin_manager_path = self.settings.get("skin_manager_path", "")
		if not self.skin_manager_path or not Path(self.skin_manager_path).exists():
			self.skin_manager_path = self.auto_detect_skins_path()

		if self.skin_manager_path and Path(self.skin_manager_path).exists():
			self.skin_path_label.setText(self.skin_manager_path)
			self.settings["skin_manager_path"] = self.skin_manager_path
			self.save_settings()
			self.refresh_skin_list()
		else:
			self.skin_path_label.setText("未检测到 War Thunder 涂装目录，请手动选择。")

		return widget

	def auto_detect_skins_path(self):
		"""Attempt to find War Thunder UserSkins path automatically"""
		try:
			# 1. Try to find Steam installation path from registry
			steam_path = None
			try:
				key = winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Valve\Steam")
				steam_path = winreg.QueryValueEx(key, "SteamPath")[0]
				winreg.CloseKey(key)
			except (WindowsError, FileNotFoundError):
				pass

			if not steam_path:
				# Fallback to common paths
				common_paths = [
					Path("C:/Program Files (x86)/Steam"),
					Path("C:/Program Files/Steam"),
					Path("D:/Steam"),
					Path("E:/Steam")
				]
				for p in common_paths:
					if p.exists():
						steam_path = str(p)
						break

			if not steam_path:
				return None

			# 2. Parse libraryfolders.vdf to find all libraries
			steam_path = Path(steam_path)
			vdf_path = steam_path / "steamapps" / "libraryfolders.vdf"

			libraries = [steam_path]
			if vdf_path.exists():
				try:
					with open(vdf_path, 'r', encoding='utf-8', errors='ignore') as f:
						content = f.read()
						# Regex to find "path"	"D:\\Games"
						matches = re.findall(r'"path"\s+"([^"]+)"', content)
						for match in matches:
							lib_path = Path(match)
							if lib_path.exists() and lib_path not in libraries:
								libraries.append(lib_path)
				except Exception as e:
					print(f"Error reading libraryfolders.vdf: {e}")

			# 3. Search for War Thunder in libraries
			for lib in libraries:
				wt_path = lib / "steamapps" / "common" / "War Thunder"
				if wt_path.exists():
					user_skins = wt_path / "UserSkins"
					if user_skins.exists():
						return str(user_skins)

			return None

		except Exception as e:
			print(f"Auto-detect error: {e}")
			return None

	def refresh_skin_list(self):
		if not self.skin_manager_path or not Path(self.skin_manager_path).exists():
			QMessageBox.warning(self, "错误", "涂装目录无效，请重新选择。")
			return

		self.skin_list_widget.clear()
		try:
			skin_dir = Path(self.skin_manager_path)
			files = sorted(skin_dir.iterdir(), key=lambda p: p.name)
			for f in files:
				# Check if disabled (ends with .disabled)
				is_disabled = f.name.endswith('.disabled')
				# Create list item
				list_item = QListWidgetItem()
				list_item.setSizeHint(QSize(0, 45)) # Height matches SkinListItem
				self.skin_list_widget.addItem(list_item)

				# Create custom widget
				item_widget = SkinListItem(str(f), is_disabled)

				# Connect signals using functools.partial to avoid lambda closure issues
				item_widget.deleteRequested.connect(partial(self.delete_skin, str(f)))
				item_widget.openRequested.connect(partial(self.open_skin_file, str(f)))
				item_widget.toggleRequested.connect(partial(self.toggle_skin_status, str(f)))

				self.skin_list_widget.setItemWidget(list_item, item_widget)
		except Exception as e:
			QMessageBox.warning(self, "错误", f"读取文件列表失败: {e}")

	def filter_skins(self, letter):
		"""Filter    list based on the first letter"""
		for i in range(self.skin_list_widget.count()):
			item = self.skin_list_widget.item(i)
			widget = self.skin_list_widget.itemWidget(item)
			if widget:
				name = widget.file_path.name
				first_char = name[0].upper()
				if letter == '#':
					# Show numbers and special chars
					item.setHidden(not first_char.isdigit())
				else:
					item.setHidden(first_char != letter)

	def handle_skin_drop(self, file_paths):
		"""Handle files dropped into the list"""
		if not self.skin_manager_path or not Path(self.skin_manager_path).exists():
			QMessageBox.warning(self, "错误", "请先设置有效的涂装目录。")
			return

		target_dir = Path(self.skin_manager_path)
		count = 0
		for path_str in file_paths:
			src = Path(path_str)
			if src.is_file():
				try:
					shutil.copy2(src, target_dir / src.name)
					count += 1
				except Exception as e:
					print(f"Copy failed: {e}")

		if count > 0:
			QMessageBox.information(self, "导入成功", f"已成功导入 {count} 个文件。")
			self.refresh_skin_list()
		else:
			QMessageBox.warning(self, "导入失败", "没有文件被导入。")

	def select_skin_folder(self):
		path = QFileDialog.getExistingDirectory(self, "选择 War Thunder UserSkins 文件夹")
		if path:
			self.skin_manager_path = path
			self.skin_path_label.setText(path)
			self.settings["skin_manager_path"] = path
			self.save_settings()
			self.refresh_skin_list()

	def delete_skin(self, file_path):
		# Use Custom Confirm Dialog
		dialog = ConfirmDialog(self, title="确认删除", message=f"确定要永久删除文件:\n{Path(file_path).name} ?")
		if dialog.exec_() == QDialog.Accepted:
			try:
				Path(file_path).unlink()
				# Use QTimer to defer UI refresh to avoid crash
				QTimer.singleShot(100, self.refresh_skin_list)
			except Exception as e:
				QMessageBox.warning(self, "错误", f"删除失败: {e}")

	def open_skin_file(self, file_path):
		if file_path and Path(file_path).exists():
			QDesktopServices.openUrl(QUrl.fromLocalFile(str(file_path)))

	def toggle_skin_status(self, file_path):
		"""Toggle between enabled and disabled by renaming"""
		try:
			p = Path(file_path)
			if p.name.endswith('.disabled'):
				# Enable: remove .disabled
				new_name = p.name[:-9] # len('.disabled') is 9
				new_path = p.parent / new_name
				p.rename(new_path)
			else:
				# Disable: add .disabled
				new_path = p.parent / (p.name + '.disabled')
				p.rename(new_path)

			# Use QTimer to defer UI refresh to avoid crash
			QTimer.singleShot(100, self.refresh_skin_list)
		except Exception as e:
			QMessageBox.warning(self, "错误", f"状态切换失败: {e}")

	def _update_pbr_split_file_list(self, file_paths, add_only=False):
		valid_files = {Path(p) for p in file_paths if os.path.isfile(p) and p.lower().endswith('.dds')}
		if add_only:
			self.pbr_split_files.update(valid_files)
		else:
			self.pbr_split_files = valid_files
		self.pbr_split_file_list.clear()
		if self.pbr_split_files:
			sorted_files = sorted(list(self.pbr_split_files), key=lambda p: p.name)
			self.pbr_split_file_list.addItems([p.name for p in sorted_files])

	def _update_image_to_dds_file_list(self, file_paths, add_only=False):
		valid_files = {Path(p) for p in file_paths if os.path.isfile(p) and
		               any(p.lower().endswith(ext) for ext in IMAGE_EXTS)}
		if add_only:
			self.image_to_dds_files.update(valid_files)
		else:
			self.image_to_dds_files = valid_files
		self.image_to_dds_file_list.clear()
		if self.image_to_dds_files:
			sorted_files = sorted(list(self.image_to_dds_files), key=lambda p: p.name)
			self.image_to_dds_file_list.addItems([p.name for p in sorted_files])

	def select_dds_files(self):
		files, _ = QFileDialog.getOpenFileNames(self, "选择DDS文件", "", "DDS Files (*.dds)")
		if files:
			self._update_pbr_split_file_list(files, add_only=True)

	def select_image_files(self):
		files, _ = QFileDialog.getOpenFileNames(self, "选择图片文件", "",
		                                        "Image Files (*.png *.tga *.jpg *.jpeg)")
		if files:
			self._update_image_to_dds_file_list(files, add_only=True)

	def handle_pbr_split_drop(self, paths):
		self._update_pbr_split_file_list(paths, add_only=True)

	def handle_image_to_dds_drop(self, paths):
		self._update_image_to_dds_file_list(paths, add_only=True)

	def clear_pbr_split_list(self):
		self._update_pbr_split_file_list([], add_only=False)

	def clear_image_to_dds_list(self):
		self._update_image_to_dds_file_list([], add_only=False)

	def select_folder(self, name, type, label):
		path = QFileDialog.getExistingDirectory(self, f"选择{type}文件夹")
		if path:
			self.settings[f"{name}_{type}_path"] = path
			if name == "pbr_split" or name == "image_to_dds":
				label.setText(f"输出文件夹 (保存{'图片' if name == 'pbr_split' else 'DDS'}): {path}")
			else:
				label.setText(f"{type.capitalize()}文件夹: {path}")
			self.save_settings()

	def invoke_in_main_thread(self, func):
		self.invoke_signal.emit(func)

	def _invoke(self, func):
		func()

	def show_help_dialog(self):
		dialog = HelpDialog(None)
		dialog.exec_()

	def show_settings_dialog(self):
		dialog = SettingsDialog(self, self.settings)
		dialog.exec_()

	def load_settings(self):
		try:
			if SETTINGS_FILE.exists():
				with open(SETTINGS_FILE, "r", encoding="utf-8") as f: return json.load(f)
		except (json.JSONDecodeError, IOError) as e:
			print(f"加载设置失败: {e}")
		return {"auto_update": True}

	def save_settings(self):
		try:
			SETTINGS_DIR.mkdir(parents=True, exist_ok=True)
			with open(SETTINGS_FILE, "w", encoding="utf-8") as f:
				json.dump(self.settings, f, indent=2)
		except IOError as e:
			print(f"保存设置失败: {e}")

	def closeEvent(self, event):
		self.save_settings()
		event.accept()

	def check_update_in_background(self):
		checker = SettingsDialog(self, self.settings)
		checker.check_update()

	def run_mipmap(self):
		widgets = self.mipmap_widgets
		log, progress = widgets["log"], widgets["progress"]
		input_path = self.settings.get("mipmap_input_path", "")
		output_path = self.settings.get("mipmap_output_path", "")
		if not all([input_path, output_path]):
			log.append("错误: 请先选择输入和输出文件夹。")
			return
		input_folder, output_folder = Path(input_path), Path(output_path)
		output_folder.mkdir(exist_ok=True)
		mipmap_files = []
		for i in range(1000):
			for ext in IMAGE_EXTS:
				file_path = input_folder / f"p{i}{ext}"
				if file_path.exists():
					mipmap_files.append(file_path)
					break
		if not mipmap_files:
			log.append("错误: 输入文件夹中未找到任何p0, p1, p2...文件。")
			return
		mipmap_files.sort(key=lambda x: int(re.search(r'p(\d+)', x.stem).group(1)))
		log.clear()
		progress.setValue(0)
		log.append(f"找到 {len(mipmap_files)} 个mipmap文件，开始处理...")
		alpha_choice = widgets["alpha_combo"].currentText()
		dds_format = widgets["dds_combo"].currentText()

		def do_run():
			try:
				mipmap_images = []
				for file_path in mipmap_files:
					img = Image.open(file_path)
					if img.mode != 'RGBA':
						img = img.convert('RGBA')
					mipmap_images.append(img)
				if alpha_choice == "纯黑":
					for img in mipmap_images:
						alpha = Image.new('L', img.size, 0)
						img.putalpha(alpha)
				elif alpha_choice == "纯白":
					for img in mipmap_images:
						alpha = Image.new('L', img.size, 255)
						img.putalpha(alpha)
				output_file = output_folder / "Mipmap.dds"
				log.append(f"正在保存DDS文件: {output_file}")

				def progress_callback(percent):
					self.invoke_in_main_thread(lambda p=percent: progress.setValue(p))

				save_dds_with_mipmaps(str(output_file), mipmap_images, dds_format, progress_callback)
				self.invoke_in_main_thread(lambda: self.finish_process(log, progress, True,
				                                                       f"成功保存DDS文件: {output_file}\n包含 {len(mipmap_images)} 级mipmap"))
			except Exception as e:
				import traceback
				traceback.print_exc()
				self.invoke_in_main_thread(lambda: self.finish_process(log, progress, False, str(e)))

		threading.Thread(target=do_run, daemon=True).start()

	def run_image_to_dds(self):
		widgets = self.image_to_dds_widgets
		log, progress = widgets["log"], widgets["progress"]
		output_path = self.settings.get("image_to_dds_output_path", "")
		if not output_path:
			log.append("错误: 请先选择输出文件夹。")
			return
		output_folder = Path(output_path)
		output_folder.mkdir(exist_ok=True)
		if not self.image_to_dds_files:
			log.append("错误: 请先选择或拖放要转换的图片文件。")
			return
		log.clear()
		progress.setValue(0)
		image_files = sorted(list(self.image_to_dds_files), key=lambda p: p.name)
		alpha_choice = widgets["alpha_combo"].currentText()
		dds_format = widgets["dds_combo"].currentText()
		log.append(f"找到 {len(image_files)} 个图片文件，开始转换...")

		def do_convert():
			success_count = 0
			for idx, image_path in enumerate(image_files):
				try:
					self.invoke_in_main_thread(lambda p=image_path.name: log.append(f"\n正在处理: {p}"))
					img = Image.open(image_path)
					output_file = output_folder / f"{image_path.stem}.dds"
					alpha_color = None
					if alpha_choice == "纯黑":
						alpha_color = "black"
					elif alpha_choice == "纯白":
						alpha_color = "white"
					save_single_dds(str(output_file), img, dds_format, alpha_color)
					self.invoke_in_main_thread(lambda p=image_path.name: log.append(f"  - {p} 已转换为DDS格式。"))
					success_count += 1
				except Exception as e:
					self.invoke_in_main_thread(
						lambda p=image_path.name, err=e: log.append(f"失败: {p} 转换失败: {err}"))
				progress_val = int((idx + 1) / len(image_files) * 100)
				self.invoke_in_main_thread(lambda v=progress_val: progress.setValue(v))
			self.invoke_in_main_thread(
				lambda: log.append(f"\n全部完成！成功 {success_count} / {len(image_files)} 个文件。"))

		threading.Thread(target=do_convert, daemon=True).start()

	def run_merge(self):
		widgets = self.pbr_widgets
		log, progress = widgets["log"], widgets["progress"]
		input_path, output_path = self.settings.get("pbr_input_path", ""), self.settings.get("pbr_output_path", "")
		if not all([input_path, output_path]): log.append("错误: 请先选择输入和输出文件夹。"); return
		input_folder, output_folder = Path(input_path), Path(output_path)
		output_folder.mkdir(exist_ok=True)
		log.clear();
		progress.setValue(0)
		groups = find_texture_groups(input_folder)
		if not groups: log.append("错误: 未找到任何完整的贴图组 (需要BaseColor, Roughness, Metallic, Normal)。"); return
		alpha_color = 'black' if widgets["alpha_combo"].currentText() == "纯黑" else 'white'
		dds_flags = 'DXT5' if widgets["dds_combo"].currentText() == "DTX5" else 'R8G8B8A8_UNORM'
		log.append(f"找到 {len(groups)} 个贴图组，开始处理 (纯Python模式)...")

		def do_run():
			success_count = 0
			for idx, (prefix, texs) in enumerate(groups.items()):
				try:
					self.invoke_in_main_thread(lambda p=prefix: log.append(f"\n正在处理组: {p}"))
					c_dds_path, n_dds_path = output_folder / f"{prefix}_c.dds", output_folder / f"{prefix}_n.dds"
					process_base_color(texs['Basecolor'], c_dds_path, alpha_color, dds_flags)
					process_roughness_metallic_normal(texs['Roughness'], texs['Metallic'], texs['Normal'], n_dds_path,
					                                  dds_flags)
					self.invoke_in_main_thread(lambda p=prefix: log.append(f"成功: {p} 已处理。"))
					success_count += 1
				except Exception as e:
					self.invoke_in_main_thread(lambda p=prefix, err=e: log.append(f"失败: {p} 处理失败: {err}"))
				self.invoke_in_main_thread(lambda val=int((idx + 1) / len(groups) * 100): progress.setValue(val))
			self.invoke_in_main_thread(lambda: log.append(f"\n全部完成！成功 {success_count} / {len(groups)} 组。"))

		threading.Thread(target=do_run, daemon=True).start()

	def run_split(self):
		widgets = self.pbr_split_widgets
		log, progress = widgets["log"], widgets["progress"]
		output_path = self.settings.get("pbr_split_output_path", "")
		if not output_path: log.append("错误: 请先选择输出文件夹。"); return
		output_folder = Path(output_path)
		output_folder.mkdir(exist_ok=True)
		log.clear();
		progress.setValue(0)
		dds_files = sorted(list(self.pbr_split_files), key=lambda p: p.name)
		if not dds_files: log.append("错误: 请先选择或拖放要处理的DDS文件。"); return
		export_alpha = widgets["export_alpha_chk"].isChecked()
		export_format = widgets["format_combo"].currentText().lower()
		self.settings["pbr_split_export_alpha"] = export_alpha
		self.settings["pbr_split_export_format"] = export_format.upper()
		self.save_settings()
		log.append(f"找到 {len(dds_files)} 个DDS文件，开始拆分...")

		def do_split():
			success_count = 0
			for idx, dds_path in enumerate(dds_files):
				try:
					prefix = dds_path.stem[:-2]
					self.invoke_in_main_thread(lambda p=prefix: log.append(f"\n正在处理: {dds_path.name}"))
					if dds_path.stem.endswith('_c'):
						split_c_texture(dds_path, output_folder, prefix, export_alpha, export_format)
						self.invoke_in_main_thread(
							lambda p=prefix, f=export_format: log.append(f"  - {p}_BaseColor.{f} 已导出。"))
						if export_alpha: self.invoke_in_main_thread(
							lambda p=prefix, f=export_format: log.append(f"  - {p}_Alpha.{f} 已导出。"))
					elif dds_path.stem.endswith('_n'):
						split_n_texture(dds_path, output_folder, prefix, export_format)
						self.invoke_in_main_thread(
							lambda p=prefix, f=export_format: log.append(f"  - {p}_Roughness.{f} 已导出。"))
						self.invoke_in_main_thread(
							lambda p=prefix, f=export_format: log.append(f"  - {p}_Metallic.{f} 已导出。"))
						self.invoke_in_main_thread(
							lambda p=prefix, f=export_format: log.append(f"  - {p}_Normal.{f} 已导出。"))
					success_count += 1
				except Exception as e:
					self.invoke_in_main_thread(lambda p=dds_path.name, err=e: log.append(f"失败: {p} 处理失败: {err}"))
				self.invoke_in_main_thread(lambda val=int((idx + 1) / len(dds_files) * 100): progress.setValue(val))
			self.invoke_in_main_thread(
				lambda: log.append(f"\n全部完成！成功 {success_count} / {len(dds_files)} 个文件。"))

		threading.Thread(target=do_split, daemon=True).start()

	def finish_process(self, log, progress, success, message):
		progress.setRange(0, 100);
		progress.setValue(100)
		if success:
			log.append("操作成功。")
			if message: log.append(f"信息:\n{message}")
		else:
			log.append("操作失败。")
			if message: log.append(f"错误信息:\n{message}")


# Texture Processing
def process_base_color(base_color_path, output_path, alpha_color='black', dds_flags='DXT5'):
	try:
		base_color = Image.open(base_color_path).convert('RGBA')
		base_color_np = np.array(base_color)
		base_color_np[:, :, 3] = 255 if alpha_color == 'white' else 0
		imageio.imwrite(str(output_path), base_color_np, format='DDS', flags=dds_flags)
	except Exception as e:
		print(f"处理 BaseColor 失败: {e}")


def process_roughness_metallic_normal(roughness_path, metallic_path, normal_path, output_path, dds_flags='DXT5'):
	try:
		roughness, metallic, normal = Image.open(roughness_path).convert('L'), Image.open(metallic_path).convert(
			'L'), Image.open(normal_path).convert('RGBA')
		if not (roughness.size == metallic.size == normal.size): raise ValueError(
			f"贴图尺寸不一致: R({roughness.size}), M({metallic.size}), N({normal.size})")
		roughness_np, metallic_np, normal_np = 255 - np.array(roughness), np.array(metallic), np.array(normal)
		combined = np.zeros_like(normal_np)
		combined[..., 0], combined[..., 1], combined[..., 2], combined[..., 3] = roughness_np, normal_np[
			...,1], metallic_np, normal_np[..., 0]
		imageio.imwrite(str(output_path), combined, format='DDS', flags=dds_flags)
	except Exception as e:
		print(f"处理 Roughness, Metallic, Normal 失败: {e}")


def split_c_texture(dds_path, output_folder, prefix, export_alpha, export_format='png'):
	try:
		img_data = imageio.imread(dds_path)
		Image.fromarray(img_data[:, :, :3]).save(output_folder / f"{prefix}_BaseColor.{export_format}")
		if export_alpha and img_data.shape[2] == 4:
			Image.fromarray(img_data[:, :, 3]).save(output_folder / f"{prefix}_Alpha.{export_format}")
	except Exception as e:
		print(f"拆分 C 纹理失败: {e}")


def split_n_texture(dds_path, output_folder, prefix, export_format='png'):
	try:
		img_data = imageio.imread(dds_path)
		if img_data.shape[2] != 4: raise ValueError(f"{dds_path.name} 不是有效的4通道_n贴图。")
		Image.fromarray(255 - img_data[:, :, 0]).save(output_folder / f"{prefix}_Roughness.{export_format}")
		Image.fromarray(img_data[:, :, 2]).save(output_folder / f"{prefix}_Metallic.{export_format}")
		h, w = img_data.shape[:2]
		normal_np = np.zeros((h, w, 4), dtype=np.uint8)
		normal_np[:, :, 0], normal_np[:, :, 1], normal_np[:, :, 2], normal_np[:, :, 3] = img_data[:, :, 3], img_data[
		                                                                                                    :,
		                                                                                                    1], 255, 255
		Image.fromarray(normal_np).save(output_folder / f"{prefix}_Normal.{export_format}")
	except Exception as e:
		print(f"拆分 N 纹理失败: {e}")


# Stylesheet
def set_global_stylesheet(app):
	stylesheet = f"""
    QWidget {{ color: {TEXT_PRIMARY}; font-family: "Microsoft YaHei UI", "Segoe UI", Arial, sans-serif; font-size: 10pt; }}
    #MainContainer {{ background-color: {COLOR_BACKGROUND_DARKEST}; border-top-left-radius: 15px; border-top-right-radius: 15px; }}
    #TitleBar {{ background-color: {COLOR_BACKGROUND_DARK}; border-top-left-radius: 15px; border-top-right-radius: 15px; }}
    #TitleLabel {{ font-weight: bold; font-size: 11pt; padding-left: 5px; }}
    #TitleBarButton, #CloseButton {{ background-color: transparent; border: none; border-radius: 5px; }}
    #TitleBarButton:hover {{ background-color: {COLOR_BORDER}; }}
    #TitleBarButton:pressed {{ background-color: {COLOR_BACKGROUND_DARKEST}; }}
    #CloseButton:hover {{ background-color: {CLOSE_BUTTON_HOVER}; }}
    #CloseButton:pressed {{ background-color: #ec4a58; }}
    #NavBar {{ background-color: {COLOR_BACKGROUND_DARK}; border: none; border-right: 1px solid {COLOR_BORDER}; padding: 5px 0; }}
    #NavBar::item {{ padding: 10px 15px; border: none; margin: 3px 8px; }}
    #NavBar::item:selected {{ background-color: {COLOR_PRIMARY}; color: {TEXT_BRIGHT}; border-radius: 5px; outline: none; }}
    #NavBar::item:hover:!selected {{ background-color: {COLOR_BACKGROUND_MEDIUM}; border-radius: 5px; }}
    QGroupBox {{ background-color: {COLOR_BACKGROUND_DARK}; border: 1px solid {COLOR_BORDER}; border-radius: 5px; margin-top: 10px; padding: 10px 5px 5px 5px; }}
    QGroupBox::title {{ subcontrol-origin: margin; subcontrol-position: top left; left: 10px; padding: 0 5px; color: {TEXT_SECONDARY}; font-weight: bold; }}
    QLineEdit, QComboBox, QSpinBox, QListWidget {{ background-color: {COLOR_BACKGROUND_MEDIUM}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; padding: 6px 8px; }}
    QTextEdit {{ background-color: {COLOR_BACKGROUND_MEDIUM}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; }}
    QLineEdit:focus, QTextEdit:focus, QComboBox:focus, QSpinBox:focus, QListWidget:focus {{ border: 1px solid {COLOR_PRIMARY}; }}
    QComboBox::drop-down {{ border: none; }}
    QComboBox::down-arrow {{ image: url(:/qt-project.org/styles/commonstyle/images/down-arrow-disabled.png); }}
    QComboBox QAbstractItemView {{ background-color: {COLOR_BACKGROUND_MEDIUM}; border: 1px solid {COLOR_BORDER}; selection-background-color: {COLOR_PRIMARY}; color: {TEXT_PRIMARY}; padding: 4px; }}
    QPushButton {{ background-color: {COLOR_BACKGROUND_MEDIUM}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; padding: 8px 15px; }}
    QPushButton:hover {{ background-color: {COLOR_HOVER_LIGHT}; }}
    QPushButton:pressed {{ background-color: {COLOR_BACKGROUND_DARKEST}; }}
    #RunButton {{ background-color: {COLOR_PRIMARY}; color: {TEXT_BRIGHT}; font-weight: bold; font-size: 12pt; }}
    #RunButton:hover {{ background-color: {COLOR_PRIMARY_HOVER}; }}
    #StatusBarButton {{ background-color: transparent; border: none; padding: 4px; border-radius: 4px; }}
    #StatusBarButton:hover {{ background-color: {COLOR_BORDER}; }}
    QProgressBar {{ border: 1px solid {COLOR_BORDER}; border-radius: 4px; text-align: center; background-color: {COLOR_BACKGROUND_MEDIUM}; }}
    QProgressBar::chunk {{ background-color: {COLOR_PRIMARY}; border-radius: 3px; }}
    QSlider::groove:horizontal {{ border: 1px solid {COLOR_BACKGROUND_DARK}; height: 8px; background: {COLOR_BACKGROUND_MEDIUM}; margin: 2px 0; border-radius: 4px; }}
    QSlider::handle:horizontal {{ background: {TEXT_PRIMARY}; border: 1px solid {TEXT_PRIMARY}; width: 18px; margin: -5px 0; border-radius: 9px; }}
    QSlider::handle:horizontal:hover {{ background: {TEXT_BRIGHT}; border: 1px solid {TEXT_BRIGHT}; }}
    QCheckBox {{ spacing: 8px; }}
    QCheckBox::indicator {{ width: 16px; height: 16px; background-color: {COLOR_BACKGROUND_MEDIUM}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; }}
    QCheckBox::indicator:hover {{ border: 1px solid #45474b; }}
    QCheckBox::indicator:checked {{ background-color: {COLOR_PRIMARY}; border: 1px solid {COLOR_PRIMARY}; }}
    #StatusBar {{ background-color: {COLOR_BACKGROUND_DARK}; border-top: 1px solid {COLOR_BORDER}; border-bottom-left-radius: 15px; border-bottom-right-radius: 15px; }}
    #StatusBar QLabel {{ padding: 0 8px; font-weight: bold; }}
    #StatusBar::item {{ border: none; }}
    #PreviewArea {{ border: 1px dashed {COLOR_BORDER}; border-radius: 5px; background-color: {COLOR_BACKGROUND_MEDIUM}; }}
    #DropTarget[drop-active="true"] {{ border: 2px dashed {COLOR_PRIMARY}; background-color: rgba(40, 42, 46, 0.7); }}
    #CustomDialog, #DialogContainer {{ background-color: {COLOR_BACKGROUND_DARKEST}; border: 1px solid {COLOR_BORDER}; border-radius: 8px; }}
    #DialogTitleBar {{ background-color: {COLOR_BACKGROUND_DARK}; border-bottom: 1px solid {COLOR_BORDER}; border-top-left-radius: 8px; border-top-right-radius: 8px; }}
    QMessageBox, QColorDialog {{ background-color: {COLOR_BACKGROUND_DARK}; }}
    QScrollBar:vertical {{ border: none; background: {COLOR_BACKGROUND_DARK}; width: 10px; margin: 0px 0px 0px 0px; }}
    QScrollBar::handle:vertical {{ background: {COLOR_BORDER}; min-height: 20px; border-radius: 5px; }}
    QScrollBar::handle:vertical:hover {{ background: #45474b; }}
    QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {{ border: none; background: none; }}
    QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical {{ background: none; }}
    QScrollBar:horizontal {{ border: none; background: {COLOR_BACKGROUND_DARK}; height: 10px; margin: 0px 0px 0px 0px; }}
    QScrollBar::handle:horizontal {{ background: {COLOR_BORDER}; min-width: 20px; border-radius: 5px; }}
    QScrollBar::handle:horizontal:hover {{ background: #45474b; }}
    QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal {{ border: none; background: none; }}
    QScrollBar::add-page:horizontal, QScrollBar::sub-page:horizontal {{ background: none; }}

    /* Skin Manager Styles */
    #AZBar {{ background-color: {COLOR_BACKGROUND_DARK}; border-right: 1px solid {COLOR_BORDER}; }}
    #AZButton {{ background-color: transparent; border: none; color: {TEXT_SECONDARY}; font-size: 9pt; font-weight: bold; }}
    #AZButton:hover {{ color: {TEXT_PRIMARY}; background-color: {COLOR_BACKGROUND_MEDIUM}; border-radius: 3px; }}
    #SkinListItem {{ background-color: {COLOR_BACKGROUND_MEDIUM}; border: 1px solid {COLOR_BORDER}; border-radius: 4px; }}
    #SkinListItem:hover {{ background-color: {COLOR_HOVER_LIGHT}; }}
    #SkinActionBtn {{ background-color: transparent; border: none; border-radius: 3px; }}
    #SkinActionBtn:hover {{ background-color: {COLOR_BORDER}; }}
    #SkinToggleBtn {{ background-color: transparent; border: none; border-radius: 3px; }}
    #SkinToggleBtn:hover {{ background-color: {COLOR_BORDER}; }}
    """
	app.setStyleSheet(stylesheet)


# Entry Point
if __name__ == '__main__':
	app = QApplication(sys.argv)
	set_global_stylesheet(app)
	window = AIASMainWindow()
	window.show()
	sys.exit(app.exec_())
