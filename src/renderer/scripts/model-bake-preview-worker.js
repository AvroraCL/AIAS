self.onmessage = async event => {
  const { id, url, expectedBytes } = event.data;
  try {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const buffer = await response.arrayBuffer();
    if (expectedBytes != null && buffer.byteLength !== expectedBytes) {
      throw new Error(`预览数据大小不匹配（${buffer.byteLength}/${expectedBytes}）`);
    }
    self.postMessage({ id, buffer }, [buffer]);
  } catch (error) {
    self.postMessage({ id, error: String(error?.message || error) });
  }
};
