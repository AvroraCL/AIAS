import { decodeDdsThumbnail } from './blk-dds.mjs';

self.onmessage = async event => {
  const { id, file } = event.data;
  try {
    const { width, height, pixels } = await decodeDdsThumbnail(file);
    const canvas = new OffscreenCanvas(width, height);
    canvas.getContext('2d').putImageData(new ImageData(pixels, width, height), 0, 0);
    const blob = await canvas.convertToBlob({ type: 'image/png' });
    self.postMessage({ id, blob });
  } catch (error) {
    self.postMessage({ id, error: error.message || String(error) });
  }
};
