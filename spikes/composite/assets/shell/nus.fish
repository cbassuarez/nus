# nus shell integration for fish. fish 4 already emits OSC 133; this adds
# the working directory and covers fish 3.
if test "$NUS_SHELL_INTEGRATION" = off; exit; end
set -g __nus_integrated 1
function __nus_cwd --on-variable PWD; printf '\e]7;file://%s%s\a' (hostname) "$PWD"; end
__nus_cwd
if not functions -q __nus_orig_prompt
    if functions -q fish_prompt; functions -c fish_prompt __nus_orig_prompt; else; function __nus_orig_prompt; printf '%s> ' (prompt_pwd); end; end
end
function fish_prompt
    set -l err $status
    if set -q __nus_ran; printf '\e]133;D;%s\a' $err; set -e __nus_ran; end
    printf '\e]133;A\a'
    __nus_orig_prompt
    printf '\e]133;B\a'
end
function __nus_preexec --on-event fish_preexec; set -g __nus_ran 1; printf '\e]133;C\a'; end
