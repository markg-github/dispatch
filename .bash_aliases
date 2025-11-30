# ~/.bash_aliases

# Set base path
DISPATCH_BIN="$HOME/code/git/markg-github/dispatch/target/debug/dispatch"

DEFAULT_OPTS=(-t devel --quiet)

# alias tail='tail -n 500 -f'
# alias dispatch='~/code/git/markg-github/dispatch/target/debug/dispatch -o markg-github -r sev-certify -t devel --quiet'
alias mgdisp="${DISPATCH_BIN} ${DEFAULT_OPTS[@]} --avahi-suffix markg-github"

tailn() {
    if [ -z "$1" ]; then
        echo "Usage: tailn [lines] <file>"
        echo "  file: required filename"
        echo "  lines: optional number of lines (default: 500)"
        return 1
    fi
    if [ -z "$2" ]; then
        tail -n 500 -f "$1"
        # tail -n "$1" -f
    else
        tail -n "$1" -f "${@:2}"
        # tail -n "$1" -f
    fi
}

# DISPATCH_COMMAND="$DISPATCH_BIN $DEFAULT_SEV_CERTIFY_REPO_OPTS"

dtsuffix() {
    if [ -z "$1" ]; then
        echo "Usage: dtsuffix <suffix> [port]"
        return 1
    fi
    "$DISPATCH_BIN" "${DEFAULT_OPTS[@]}" -o markg-github -r sev-certify --avahi-suffix "$1" -b "0.0.0.0:${2:-8080}"
}

dt() {
    "$DISPATCH_BIN" "${DEFAULT_OPTS[@]}" -o markg-github -r sev-certify -b "0.0.0.0:${1:-8080}"
}



