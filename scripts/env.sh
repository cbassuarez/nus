# source this: sets CEF_PATH and the loader path so spikes and the app find libcef.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CEF_PATH="$root/vendor/cef"
case "$(uname -s)" in
  Darwin) export DYLD_FALLBACK_LIBRARY_PATH="${DYLD_FALLBACK_LIBRARY_PATH:+$DYLD_FALLBACK_LIBRARY_PATH:}$CEF_PATH:$CEF_PATH/Chromium Embedded Framework.framework/Libraries" ;;
  Linux)  export LD_LIBRARY_PATH="${LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}$CEF_PATH" ;;
  *)      export PATH="$PATH:$CEF_PATH:/c/Program Files/Microsoft Visual Studio/2022/Community/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja" ;;
esac
echo "CEF_PATH=$CEF_PATH"
