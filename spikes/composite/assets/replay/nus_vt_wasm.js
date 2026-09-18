export class Shell {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ShellFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_shell_free(ptr, 0);
    }
    /**
     * True when the cursor sits at a shell prompt, from the OSC 133 marks in
     * the stream — how the app tells a command line from program output.
     * @returns {boolean}
     */
    get at_prompt() {
        const ret = wasm.shell_at_prompt(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Glyph bitmaps rasterised during the last `draw`, packed as described on
     * the field. Empty once the atlas has warmed up.
     * @returns {number}
     */
    get atlas() {
        const ret = wasm.shell_atlas(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get atlas_len() {
        const ret = wasm.shell_atlas_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get atlas_size() {
        const ret = wasm.shell_atlas_size(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get cell_h() {
        const ret = wasm.shell_cell_h(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get cell_w() {
        const ret = wasm.shell_cell_w(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get cols() {
        const ret = wasm.shell_cols(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get default_bg() {
        const ret = wasm.shell_default_bg(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Build the frame. Afterwards `instances` holds the renderer's draw list
     * and `take_atlas` holds any glyphs it rasterised on the way.
     * @param {boolean} focused
     */
    draw(focused) {
        wasm.shell_draw(this.__wbg_ptr, focused);
    }
    /**
     * Feed raw PTY bytes — exactly the bytes a shell wrote.
     * @param {Uint8Array} bytes
     */
    feed(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.shell_feed(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Pointer to the flattened draw list; JS wraps it in a `Float32Array`.
     * Each instance is `[x, y, w, h, u0, v0, u1, v1, r, g, b, a, kind, phase]`.
     * @returns {number}
     */
    get instances() {
        const ret = wasm.shell_instances(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get instances_len() {
        const ret = wasm.shell_instances_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * `px` is the terminal font size in physical pixels — pass the CSS size
     * times the device pixel ratio, the way the app passes its scale factor.
     * @param {number} cols
     * @param {number} rows
     * @param {number} scrollback
     * @param {number} px
     */
    constructor(cols, rows, scrollback, px) {
        const ret = wasm.shell_new(cols, rows, scrollback, px);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        ShellFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @param {number} cols
     * @param {number} rows
     */
    resize(cols, rows) {
        wasm.shell_resize(this.__wbg_ptr, cols, rows);
    }
    /**
     * @returns {number}
     */
    get rows() {
        const ret = wasm.shell_rows(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Change the font size, in physical pixels. Rebuilds the cell metrics the
     * way the app does when the window's scale factor changes.
     * @param {number} px
     */
    set_px(px) {
        wasm.shell_set_px(this.__wbg_ptr, px);
    }
    /**
     * Repaint the palette for a theme change: the app's themes drive it the
     * same way, so paper and ink here are the real mechanism.
     * @param {number} fg
     * @param {number} bg
     * @param {number} cursor
     * @param {Uint32Array} ansi
     */
    set_theme(fg, bg, cursor, ansi) {
        const ptr0 = passArray32ToWasm0(ansi, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.shell_set_theme(this.__wbg_ptr, fg, bg, cursor, ptr0, len0);
    }
    /**
     * @returns {number}
     */
    get stride() {
        const ret = wasm.shell_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Text of the visible grid — what a selection would copy.
     * @returns {string}
     */
    text() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.shell_text(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    tick() {
        wasm.shell_tick(this.__wbg_ptr);
    }
}
if (Symbol.dispose) Shell.prototype[Symbol.dispose] = Shell.prototype.free;

/**
 * Which nus this was built from, so the page can name it.
 * @returns {string}
 */
export function source_rev() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.source_rev();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_5d9e815e6fdf150f: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_generic_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./nus_vt_wasm_bg.js": import0,
    };
}

const ShellFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_shell_free(ptr, 1));

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('nus_vt_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
