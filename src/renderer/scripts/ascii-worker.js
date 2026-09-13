import { convertAscii } from './ascii-state.mjs';
self.onmessage = ({ data }) => {
  try {
    const result = convertAscii(data);
    self.postMessage({ revision: data.revision, result }, [result.colors.buffer, result.alphas.buffer]);
  } catch (error) { self.postMessage({ revision: data.revision, error: error.message }); }
};
