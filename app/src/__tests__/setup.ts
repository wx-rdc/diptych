import '@testing-library/jest-dom/vitest';

import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/react';

// v2.3 P0：jsdom 无法执行 Vite ?worker&inline 内联 worker，测试统一 mock 构造器
vi.mock('pdfjs-dist/build/pdf.worker.min.mjs?worker&inline', () => ({
  default: class {
    postMessage() {}
    addEventListener() {}
    removeEventListener() {}
    terminate() {}
  },
}));

afterEach(() => {
  cleanup();
});
