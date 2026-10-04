# nus shell integration for bash. Sourced as the rcfile: loads yours first.
[ -f "$HOME/.bashrc" ] && . "$HOME/.bashrc"
[ "$NUS_SHELL_INTEGRATION" = off ] && return 0
[ -n "$__nus_integrated" ] && return 0
__nus_integrated=1
# One command line, one start mark (OSC 133;C) and one end mark (133;D).
# bash runs a DEBUG trap before every simple command, the prompt's own
# hooks included, so a trap alone marks a start for each part of
# `make; make test` and again just before the prompt: nus would time only
# a command's last moment. PS0 (bash 4.4+) is expanded once, after a line
# is read and before it runs; the start goes there. An empty line runs
# nothing and gets no marks.
__nus_precmd() {
    local err=$?
    if [ -n "$__nus_ran" ]; then printf '\e]133;D;%s\a' "$err"; unset __nus_ran; fi
    printf '\e]7;file://%s%s\a' "$HOSTNAME" "$PWD"
    # Hooks after this one see the command's status, not this printf's.
    return $err
}
if (( BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4) )); then
    # The subscript is arithmetic, evaluated in this shell: it sets
    # __nus_ran and expands to nothing.
    PS0=$'\e]133;C\a''${__nus_noop[__nus_ran=1]}'"${PS0}"
    PROMPT_COMMAND="__nus_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
else
    # Older bash (macOS's 3.2): the trap, armed once per prompt by the last
    # thing PROMPT_COMMAND does, so neither the prompt's hooks nor a line's
    # second command count as a start.
    __nus_preexec() {
        [ -n "$COMP_LINE" ] && return
        [ -z "$__nus_armed" ] && return
        unset __nus_armed
        # An empty line runs nothing: the first command is the prompt's own.
        [ "$BASH_COMMAND" = __nus_precmd ] && return
        __nus_ran=1
        printf '\e]133;C\a'
    }
    __nus_arm() { local err=$?; __nus_armed=1; return $err; }
    PROMPT_COMMAND="__nus_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND};__nus_arm"
    trap '__nus_preexec' DEBUG
fi
PS1="\[\e]133;A\a\]${PS1}\[\e]133;B\a\]"
