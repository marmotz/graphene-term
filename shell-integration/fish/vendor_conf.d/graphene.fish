# Graphene shell integration for fish.
#
# Graphene prepends the parent of this folder's `fish` directory to XDG_DATA_DIRS, so
# fish sources this file as vendor configuration. It makes the interactive shell tell
# Graphene when a command starts and ends; fish's own OSC 133 marks are ignored. The
# prompt is never read, wrapped or rebuilt, and the user's dotfiles are never modified.

if set -q GRAPHENE_XDG_DATA_DIRS
    set -gx XDG_DATA_DIRS $GRAPHENE_XDG_DATA_DIRS
    set -e GRAPHENE_XDG_DATA_DIRS
else
    set -e XDG_DATA_DIRS
end

# The session token stays in a shell variable: commands run from the shell never see it.
if set -q GRAPHENE_BLOCK_TOKEN
    set -g __graphene_token $GRAPHENE_BLOCK_TOKEN
    set -e GRAPHENE_BLOCK_TOKEN
end

if status is-interactive; and test -n "$__graphene_token"
    # $argv is the command line. Not emitted for an empty one.
    function __graphene_preexec --on-event fish_preexec
        set -l encoded (printf '%s' $argv | command base64 -w0 2>/dev/null)
        printf '\e]6973;%s;start;%s\e\\' $__graphene_token "$encoded"
    end

    function __graphene_postexec --on-event fish_postexec
        printf '\e]6973;%s;end;%s\e\\' $__graphene_token $status
    end
end
