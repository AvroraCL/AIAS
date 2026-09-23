export function createBlkThumbnails({ desktop, thumbnail, convertFileSrc }) {
  let worker;
  let nextId = 0;
  let generation = 0;
  const pending = new Map();
  const cache = new Map();

  function browserWorker() {
    if (worker) return worker;
    worker = new Worker(new URL('./blk-dds-worker.mjs', import.meta.url), { type: 'module' });
    worker.onmessage = event => {
      const request = pending.get(event.data.id);
      if (!request) return;
      pending.delete(event.data.id);
      request.resolve(event.data);
    };
    worker.onerror = error => {
      for (const request of pending.values()) request.resolve({ error: error.message || 'DDS 解码失败' });
      pending.clear();
      worker.terminate();
      worker = undefined;
    };
    return worker;
  }

  function reset() {
    generation++;
    worker?.terminate();
    worker = undefined;
    for (const request of pending.values()) request.resolve({ error: '已切换目录' });
    pending.clear();
    for (const entry of cache.values()) if (entry.objectUrl) URL.revokeObjectURL(entry.objectUrl);
    cache.clear();
  }

  function get(name, directory, browserFile) {
    const key = name.toLowerCase();
    if (cache.has(key)) return cache.get(key).promise;
    const current = generation;
    const entry = { objectUrl: null };
    entry.promise = (async () => {
      try {
        let url;
        if (desktop) {
          const path = `${directory.replace(/[\\/]+$/, '')}\\${name}`;
          url = convertFileSrc(await thumbnail(path));
        } else {
          if (!browserFile) throw new Error('所选目录中找不到 DDS 文件');
          const result = await new Promise(resolve => {
            const id = ++nextId;
            pending.set(id, { resolve });
            browserWorker().postMessage({ id, file: browserFile });
          });
          if (result.error) throw new Error(result.error);
          url = URL.createObjectURL(result.blob);
          entry.objectUrl = url;
        }
        if (current !== generation) {
          if (entry.objectUrl) URL.revokeObjectURL(entry.objectUrl);
          return { error: '已切换目录' };
        }
        return { url };
      } catch (error) {
        return { error: error.message || String(error) };
      }
    })();
    cache.set(key, entry);
    return entry.promise;
  }

  return { get, reset };
}
