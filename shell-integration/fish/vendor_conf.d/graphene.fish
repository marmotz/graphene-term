# Graphene shell integration for fish.
#
# Graphene prepends the parent of this folder's `fish` directory to XDG_DATA_DIRS, so
# fish sources this file as vendor configuration. fish 4.0 and later emit the OSC 133
# marks themselves (prompt end since 4.3): this file only adds the marks the running
# fish does not emit. The user's dotfiles are never modified.

if set -q GRAPHENE_XDG_DATA_DIRS
    set -gx XDG_DATA_DIRS $GRAPHENE_XDG_DATA_DIRS
    set -e GRAPHENE_XDG_DATA_DIRS
else
    set -e XDG_DATA_DIRS
end

if status is-interactive
    # Version as a number: 4.3.0 is 40300.
    set -l __graphene_parts (string match -r '^(\d+)\.(\d+)(?:\.(\d+))?' -- $version)
    set -l __graphene_version (math "$__graphene_parts[2] * 10000 + $__graphene_parts[3] * 100 + 0$__graphene_parts[4]")

    # Native marks: from 4.0, switchable with the `mark-prompt` feature since 4.0.6.
    set -l __graphene_native 0
    if test $__graphene_version -ge 40006
        status test-feature mark-prompt 2>/dev/null; and set __graphene_native 1
    else if test $__graphene_version -ge 40000
        set __graphene_native 1
    end

    if test $__graphene_native -eq 0
        # No native mark at all: emit every mark.
        set -g __graphene_emit_start 1
        set -g __graphene_emit_end 1

        function __graphene_preexec --on-event fish_preexec
            printf '\e]133;C\e\\'
        end

        function __graphene_postexec --on-event fish_postexec
            printf '\e]133;D;%s\e\\' $status
        end
    else if test $__graphene_version -lt 40300
        # Native marks without the prompt end.
        set -g __graphene_emit_start 0
        set -g __graphene_emit_end 1
    end

    if set -q __graphene_emit_end
        # Wraps fish_prompt on every prompt, since the user's configuration may define
        # or replace it after this file is sourced.
        function __graphene_prompt --on-event fish_prompt
            if test $__graphene_emit_start -eq 1
                printf '\e]133;A\e\\'
            end
            if functions -q fish_prompt
                and not functions fish_prompt | string match -q '*__graphene_user_prompt*'
                functions -e __graphene_user_prompt
                functions -c fish_prompt __graphene_user_prompt
                function fish_prompt
                    __graphene_user_prompt
                    printf '\e]133;B\e\\'
                end
            end
        end
    end
end
