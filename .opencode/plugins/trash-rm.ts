import type { Plugin, PluginInput } from "@opencode-ai/plugin"

/**
 * Intercepts `rm` commands in bash tool calls and rewrites them to `trash-put`
 * (from trash-cli). Strips destructive flags (-r, -f, --recursive, --force)
 * since trash-put is non-destructive by nature.
 */
export default (async (_input: PluginInput) => {
  return {
    "tool.execute.before": async (input, output) => {
      if (input.tool !== "bash") return

      const args = output.args as any
      let cmd: string | null = null

      if (typeof args === "string") {
        cmd = args
      } else if (args && typeof args === "object" && "command" in args) {
        cmd = args.command
      }

      if (!cmd || typeof cmd !== "string") return
      if (!/\brm\b/.test(cmd)) return

      const rewritten = cmd.replace(/(?:^|\s)(?:\S*\/)?rm(?=\s|$)/, " trash-put")
      const cleaned = rewritten
        .replace(/\s*-{1,2}(?:r|R|f|recursive|force)\b/gi, "")
        .replace(/\s{2,}/g, " ")
        .trim()

      if (cleaned === cmd) return

      if (typeof args === "string") {
        output.args = cleaned
      } else if (args && typeof args === "object") {
        args.command = cleaned
      }
    }
  }
}) satisfies Plugin
