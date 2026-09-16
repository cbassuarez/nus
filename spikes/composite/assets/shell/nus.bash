# nus shell integration for bash. Sourced as the rcfile: loads yours first.
[ -f "$HOME/.bashrc" ] && . "$HOME/.bashrc"
[ "$NUS_SHELL_INTEGRATION" = off ] && return 0
[ -n "$__nus_integrated" ] && return 0
__nus_integrated=1
__nus_prompt_start() { printf '\e]133;A\a'; }
__nus_prompt_end() { printf '\e]133;B\a'; }
__nus_precmd() {
    local err=$?
    if [ -n "$__nus_ran" ]; then printf '\e]133;D;%s\a' "$err"; unset __nus_ran; fi
    printf '\e]7;file://%s%s\a' "$HOSTNAME" "$PWD"
}
__nus_preexec() {
    [ -n "$COMP_LINE" ] && return
    [ -n "$__nus_in_prompt" ] && return
    __nus_ran=1
    printf '\e]133;C\a'
}
PROMPT_COMMAND="__nus_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
PS1="\[\e]133;A\a\]${PS1}\[\e]133;B\a\]"
trap '__nus_preexec' DEBUG
