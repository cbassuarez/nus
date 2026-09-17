//! Which server speaks for which file. The table is the common set; the
//! host can add rows (rules, settings). Binaries resolve from the
//! profile's `bin/` (what the bundles fetch) before PATH.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// Language id as the protocol names it (`rust`, `typescript`…).
    pub language: &'static str,
    /// Extensions this server takes, without the dot.
    pub extensions: &'static [&'static str],
    /// The binary and its arguments.
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// Files that mark a workspace root, nearest one up from the file.
    pub roots: &'static [&'static str],
}

pub const SERVERS: &[Server] = &[
    Server {
        language: "rust",
        extensions: &["rs"],
        command: "rust-analyzer",
        args: &[],
        roots: &["Cargo.toml"],
    },
    Server {
        language: "typescript",
        extensions: &["ts", "tsx", "mts", "cts"],
        command: "typescript-language-server",
        args: &["--stdio"],
        roots: &["tsconfig.json", "package.json"],
    },
    Server {
        language: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        command: "typescript-language-server",
        args: &["--stdio"],
        roots: &["package.json"],
    },
    Server {
        language: "python",
        extensions: &["py", "pyi"],
        command: "pyright-langserver",
        args: &["--stdio"],
        roots: &["pyproject.toml", "setup.py", "requirements.txt"],
    },
    Server {
        language: "shellscript",
        extensions: &["sh", "bash", "zsh"],
        command: "bash-language-server",
        args: &["start"],
        roots: &[],
    },
    Server {
        language: "powershell",
        extensions: &["ps1", "psm1", "psd1"],
        command: "powershell-editor-services",
        args: &[],
        roots: &[],
    },
    Server {
        language: "go",
        extensions: &["go"],
        command: "gopls",
        args: &[],
        roots: &["go.mod"],
    },
    Server {
        language: "c",
        extensions: &["c", "h"],
        command: "clangd",
        args: &[],
        roots: &["compile_commands.json", "CMakeLists.txt"],
    },
    Server {
        language: "cpp",
        extensions: &["cpp", "cc", "cxx", "hpp", "hh"],
        command: "clangd",
        args: &[],
        roots: &["compile_commands.json", "CMakeLists.txt"],
    },
    Server {
        language: "lua",
        extensions: &["lua", "luau"],
        command: "luau-lsp",
        args: &["lsp"],
        roots: &[],
    },
    Server {
        language: "json",
        extensions: &["json", "jsonc"],
        command: "vscode-json-language-server",
        args: &["--stdio"],
        roots: &[],
    },
    Server {
        language: "css",
        extensions: &["css", "scss", "less"],
        command: "vscode-css-language-server",
        args: &["--stdio"],
        roots: &[],
    },
    Server {
        language: "html",
        extensions: &["html", "htm"],
        command: "vscode-html-language-server",
        args: &["--stdio"],
        roots: &[],
    },
    Server {
        language: "yaml",
        extensions: &["yml", "yaml"],
        command: "yaml-language-server",
        args: &["--stdio"],
        roots: &[],
    },
    Server {
        language: "toml",
        extensions: &["toml"],
        command: "taplo",
        args: &["lsp", "stdio"],
        roots: &[],
    },
    Server {
        language: "markdown",
        extensions: &["md", "markdown"],
        command: "marksman",
        args: &["server"],
        roots: &[],
    },
];

/// The server for a file, by extension.
pub fn server_for(path: &Path) -> Option<&'static Server> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    SERVERS
        .iter()
        .find(|s| s.extensions.contains(&ext.as_str()))
}

/// The language id for a file, for colouring and `didOpen`; falls back to
/// `plaintext`.
pub fn language_for(path: &Path) -> &'static str {
    server_for(path).map(|s| s.language).unwrap_or("plaintext")
}

/// The workspace root for a file: the nearest ancestor holding one of the
/// server's root markers, else the file's directory.
pub fn root_for(server: &Server, file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(file);
    for anc in dir.ancestors() {
        if server.roots.iter().any(|m| anc.join(m).exists()) {
            return anc.to_path_buf();
        }
    }
    dir.to_path_buf()
}

/// Find the binary: `bin_dir` (the profile's fetched tools) first, then
/// PATH. Windows takes `.exe`, `.cmd` and `.bat` shims.
pub fn resolve(command: &str, bin_dir: Option<&Path>) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{command}.exe"),
            format!("{command}.cmd"),
            format!("{command}.bat"),
            command.to_string(),
        ]
    } else {
        vec![command.to_string()]
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(b) = bin_dir {
        dirs.push(b.to_path_buf());
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    for d in dirs {
        for n in &names {
            let p = d.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map() {
        assert_eq!(
            server_for(Path::new("a/b.rs")).unwrap().command,
            "rust-analyzer"
        );
        assert_eq!(language_for(Path::new("x.TSX")), "typescript");
        assert_eq!(language_for(Path::new("x.unknown")), "plaintext");
        assert!(server_for(Path::new("noext")).is_none());
    }

    #[test]
    fn root_walks_up() {
        let dir = std::env::temp_dir().join(format!("nus-lsp-root-{}", std::process::id()));
        let deep = dir.join("src").join("inner");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        let s = server_for(Path::new("x.rs")).unwrap();
        assert_eq!(root_for(s, &deep.join("m.rs")), dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
