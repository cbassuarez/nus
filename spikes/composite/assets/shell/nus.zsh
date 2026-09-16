# nus shell integration for zsh: ZDOTDIR points here; your own rc loads first.
[ -f "${NUS_USER_ZDOTDIR:-$HOME}/.zshrc" ] && ZDOTDIR="${NUS_USER_ZDOTDIR:-$HOME}" . "${NUS_USER_ZDOTDIR:-$HOME}/.zshrc"
[ "$NUS_SHELL_INTEGRATION" = off ] && return 0
[ -n "$__nus_integrated" ] && return 0
__nus_integrated=1
autoload -Uz add-zsh-hook
__nus_precmd() {
    local err=$?
    if [ -n "$__nus_ran" ]; then printf '\e]133;D;%s\a' "$err"; unset __nus_ran; fi
    printf '\e]7;file://%s%s\a' "$HOST" "$PWD"
}
__nus_preexec() { __nus_ran=1; printf '\e]133;C\a'; }
add-zsh-hook precmd __nus_precmd
add-zsh-hook preexec __nus_preexec
PS1=$'%{\e]133;A\a%}'"$PS1"$'%{\e]133;B\a%}'
