// Vite 后缀导入的模块声明（tsc --noEmit 用，vite 构建时替换）
declare module 'pdfjs-dist/build/pdf.worker.min.mjs?worker&inline' {
  const WorkerCtor: new () => Worker;
  export default WorkerCtor;
}
