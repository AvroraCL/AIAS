// Keep the complete check/confirm/download lifecycle under one lock.
export function createUpdateController({ isDesktop, check, checkMirror, getVersion, ui }) {
  let busy = false;
  return async function checkForUpdates(silent = true) {
    if (busy) return;
    if (!isDesktop) {
      if (!silent) await ui.message("桌面版功能", "请在 AIAS 桌面应用中检查和安装更新。");
      return;
    }
    busy = true;
    ui.busy(true);
    let update;
    let mirror;
    try {
      update = await check({ timeout: 15000 });
      if (!update) {
        ui.available(false);
        if (!silent) ui.activity("已是最新版本", "当前版本 " + await getVersion(), "success");
        return;
      }
      ui.available(true);
      ui.activity("更新可用", update.version + " — 点击下载", "success");
      if (silent || !await ui.confirm("发现新版本", "版本 " + update.version + " 可用。\n\n是否立即下载并安装更新？")) return;
      ui.activity("正在下载更新", update.version);
      const download = async (candidate) => {
        let downloaded = 0;
        let total = 0;
        ui.status("正在下载");
        await candidate.download((event) => {
          if (event.event === "Started") {
            total = Number(event.data.contentLength) || 0;
          } else if (event.event === "Progress") {
            downloaded += Number(event.data.chunkLength) || 0;
            ui.status(total ? "下载 " + Math.min(100, Math.round(downloaded / total * 100)) + "%" : "已下载 " + ui.formatSize(downloaded));
          } else if (event.event === "Finished") {
            ui.status("正在验证更新");
          }
        }, { timeout: 600000 });
      };
      let installer = update;
      try {
        await download(update);
      } catch (error) {
        ui.activity("正在重试更新", "下载失败，检查备用源");
        mirror = await checkMirror();
        // The user approved this exact version; never silently substitute another.
        if (!mirror || mirror.version !== update.version) throw new Error("备用源尚未提供版本 " + update.version + "，请稍后重试。原始错误：" + (error.message || String(error)));
        await download(mirror);
        installer = mirror;
      }
      ui.status("正在启动安装器");
      await installer.install();
      ui.activity("更新安装器已启动", "应用将退出以完成更新", "success");
    } catch (error) {
      const message = error.message || String(error);
      ui.activity("更新失败", message, "error");
      if (!silent) {
        ui.status("更新失败");
        await ui.message("更新失败", message);
      }
    } finally {
      await Promise.allSettled([update?.close(), mirror?.close()]);
      busy = false;
      ui.busy(false);
    }
  };
}

export function scheduleUpdateCheck(isDesktop, settings, schedule, check) {
  if (isDesktop && settings.autoUpdate) {
    schedule(() => { if (settings.autoUpdate) check(true); }, 2000);
  }
}
