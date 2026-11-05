// Browser stand-in for Node built-ins that bundled libraries reference only on their Node code path
// (canvaskit-wasm's Emscripten loader calls require('fs') / require('path') when run under Node).
export {};
