/**
 * hirsel completions - Shell completion scripts
 *
 * Outputs shell completion scripts for bash or zsh.
 */

import { ICONS } from "../index";
import { dim, bold } from "../../shared/theme";

// Bash completion script
const BASH_COMPLETION = `
# Hirsel bash completion
_hirsel_completions() {
    local cur="\${COMP_WORDS[COMP_CWORD]}"
    local prev="\${COMP_WORDS[COMP_CWORD-1]}"

    # Main commands
    local commands="go view log attach msg diff deliver pause resume delete prune runs summary task-add task-delete task-done task-reopen task-unclaim tasks config templates completions man"

    case "\${prev}" in
        hirsel)
            COMPREPLY=( $(compgen -W "\${commands}" -- "\${cur}") )
            return 0
            ;;
        go|view|log|attach|msg|diff|deliver|pause|resume|delete|summary|tasks|task-add|task-delete|task-done|task-reopen|task-unclaim)
            # Complete run names
            local runs=$(ls ~/.hirsel/runs 2>/dev/null)
            COMPREPLY=( $(compgen -W "\${runs}" -- "\${cur}") )
            return 0
            ;;
        config)
            # Complete agent names
            COMPREPLY=( $(compgen -W "claude gemini opencode codex goose" -- "\${cur}") )
            return 0
            ;;
        --template)
            # Complete template names
            local templates=$(ls ~/.hirsel/templates 2>/dev/null || ls ./templates 2>/dev/null)
            COMPREPLY=( $(compgen -W "\${templates}" -- "\${cur}") )
            return 0
            ;;
        *)
            ;;
    esac

    # Options
    if [[ "\${cur}" == -* ]]; then
        COMPREPLY=( $(compgen -W "--help --json --workers --time-limit --project --template" -- "\${cur}") )
        return 0
    fi
}

complete -F _hirsel_completions hirsel
`;

// Zsh completion script
const ZSH_COMPLETION = `
#compdef hirsel

_hirsel() {
    local -a commands
    commands=(
        'go:Start a new run'
        'view:View run status'
        'log:View activity log'
        'attach:Watch worker live'
        'msg:Send message to run'
        'diff:Show code changes'
        'deliver:Create branch in target repo'
        'pause:Pause workers'
        'resume:Resume paused run'
        'delete:Remove a run'
        'prune:Remove delivered runs'
        'runs:List all runs'
        'summary:Generate run summary'
        'task-add:Add task to run'
        'task-delete:Delete task'
        'task-done:Mark task done'
        'task-reopen:Reopen completed task'
        'task-unclaim:Unclaim task'
        'tasks:List tasks'
        'config:Configure agent'
        'templates:List templates'
        'completions:Shell completions'
        'man:Show manual'
    )

    _arguments -C \\
        '1: :->command' \\
        '*:: :->args'

    case $state in
        command)
            _describe -t commands 'hirsel command' commands
            ;;
        args)
            case $words[1] in
                go|view|log|attach|msg|diff|deliver|pause|resume|delete|summary|tasks|task-add|task-delete|task-done|task-reopen|task-unclaim)
                    # Complete run names
                    local -a runs
                    runs=($(ls ~/.hirsel/runs 2>/dev/null))
                    _describe -t runs 'run name' runs
                    ;;
                config)
                    local -a agents
                    agents=('claude:Anthropic Claude Code' 'gemini:Google Gemini CLI' 'opencode:OpenCode' 'codex:OpenAI Codex CLI' 'goose:Block Goose')
                    _describe -t agents 'agent' agents
                    ;;
            esac
            ;;
    esac
}

_hirsel
`;

// Main command handler
export default async function completions(args: string[]): Promise<void> {
  const shell = args[0] || process.env.SHELL?.split("/").pop() || "";

  if (shell === "bash") {
    console.log(BASH_COMPLETION);
    return;
  }

  if (shell === "zsh") {
    console.log(ZSH_COMPLETION);
    return;
  }

  // Show instructions
  console.log(bold("\nShell Completions\n"));

  console.log(bold("Bash:"));
  console.log(dim("  Add to ~/.bashrc:"));
  console.log("    eval \"$(hirsel completions bash)\"");
  console.log();

  console.log(bold("Zsh:"));
  console.log(dim("  Add to ~/.zshrc:"));
  console.log("    eval \"$(hirsel completions zsh)\"");
  console.log();
  console.log(dim("  Or install to fpath:"));
  console.log("    hirsel completions zsh > ~/.zsh/completions/_hirsel");
  console.log();

  console.log(dim("Usage: hirsel completions <bash|zsh>"));
}
