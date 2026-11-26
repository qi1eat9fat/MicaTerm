---
name: micaterm
description: This skill should be used when the user asks to "connect to a server", "SSH into a machine", "run a command on a remote host", "check a server", "list servers", "upload/download or sync files to a server", "add, change or remove a saved host or credential", "look at or type into the terminal I handed you", or mentions MicaTerm or the `micaterm` CLI. Use `micaterm` instead of running `ssh`, `scp`, or `sftp` directly.
---

# MicaTerm CLI

Use the `micaterm` CLI to manage the user's saved SSH hosts and credentials, run one-off commands, and transfer files through the user's running MicaTerm desktop app. MicaTerm holds the passwords and keys in its vault and connects with the saved configuration.

Prefer `micaterm` over running `ssh`, `scp`, or `sftp` directly.
When unsure where you are or what you can do, run `micaterm env` first: it says which MicaTerm card you run in, which card the user handed you, whether MicaTerm and its external CLI are on, and lists every command. Do not inspect processes or read MicaTerm's files to find out. In a MicaTerm local card, `$MICATERM_CLI` is the full path of the command when `micaterm` is not on the PATH.
Run `micaterm --help` or `micaterm <command> --help` for details. Every command prints JSON when piped or given `--json`.

## Common Workflow

```bash
micaterm env
micaterm hosts list -q <keyword> --json
micaterm hosts show <host-id> --json
micaterm exec <host-id> "<command>"
micaterm upload <host-id> <local-path> <remote-path>
micaterm download <host-id> <remote-path> <local-path>
micaterm sync <host-id> <local-dir> <remote-dir> [--delete]
```

`<host-id>` is the 16-character `id` from `micaterm hosts list`.

For a simple command, pass one complete remote shell command string as `<command>`.
Pass a command through stdin when it contains quotes, pipes, redirects, JSON, `$`, backticks, or nested shell code, and quote the heredoc delimiter so the local shell leaves it alone:

```bash
micaterm exec <host-id> --stdin <<'EOF'
<command>
EOF
```

## Cards Handed to You

The user can hand you one of their remote terminal cards in MicaTerm (right-click → Hand to agent / 交给 Agent) and take it back the same way. Use it when the host allows only one login (a bastion host), or when the user wants you to work in the terminal they see.

```bash
micaterm cards list --json
micaterm exec --card <card-id> "<command>"
micaterm exec --card <card-id> --stdin <<'EOF'
<command>
EOF
micaterm cards read <card-id> --lines 200
micaterm cards send-key <card-id> q
```

- `<card-id>` is the `id` from `micaterm cards list`; it ends when the user takes the card back or closes it. Only handed cards are listed, read or typed into.
- `exec --card` types into the user's own card, in sight of the user. It sends Ctrl-C first, which interrupts whatever runs there.
- The command runs where that card is now: its user, folder and machine, after any `su`, `sudo -i` or nested `ssh`. With `--json`, the result's `context` says where: `{"user", "host", "cwd", "shell", "interp"}`; send `{"card": "<card-id>", "command": "..."}`.
- stderr comes as stdout. One command at a time per card. Only bash, dash and busybox ash.
- Programs that ask for a password wait in the user's card: use `sudo -n` and other non-interactive flags. Give your own tool a timeout; when the command is given up, it gets Ctrl-C.
- `cards read` prints the last lines of the card's screen and scrollback as the user sees them (120 by default, at most 2000), wrapped lines joined. Use it to see what a full-screen program or a prompt shows before you press anything.
- `cards send-key` presses one key: a printable character (`q`, `y`), or `ctrl-a`…`ctrl-z`, `esc`, `enter`, `tab`, `backspace`, `space`, the arrow keys, `home`, `end`, `page-up`, `page-down`, `f1`…`f12`; `--repeat N` presses it N times. While an `exec --card` command runs there, only `ctrl-c`.
- Files cannot be transferred this way; write a small text file with a heredoc in the command.

## File Transfer

```bash
micaterm upload <host-id> ./dist /tmp/dist
micaterm upload <host-id> ./app.tar.gz /tmp/release.tar.gz
micaterm download <host-id> /var/log/nginx ./logs/nginx
micaterm sync <host-id> ./dist /opt/app --delete
```

`sync` copies what the local folder holds into the remote folder, making the remote folder when it is not there (its parent must exist). Files with the same size and modification time are skipped, so running it again copies only what changed. `--delete` first removes what is in the remote folder and not in the local one. It only goes from this machine to the host, and never into `/`.

A transfer that finishes with some files failed exits with 1 and lists each failed file on stderr.

## Manage Hosts and Credentials

```bash
micaterm hosts list
micaterm hosts show <host-id>
micaterm hosts create < host.json
micaterm hosts update <host-id> < changes.json
micaterm hosts delete <host-id>
micaterm credentials list
micaterm credentials show <credential-id>
micaterm credentials create < credential.json
micaterm credentials update <credential-id> < changes.json
micaterm credentials delete <credential-id>
```

`create` and `update` read one JSON object from stdin; `update` changes only the fields given. `micaterm hosts create --help` and `micaterm credentials create --help` list the fields. What `show --json` prints can be edited and passed back to `update` as it is. For example:

```bash
micaterm hosts create <<'EOF'
{"name": "web-01", "host": "10.0.1.12", "auth": "credential", "credential": "<credential-id>"}
EOF
```

- The host list is flat: there are no groups.
- A host logs in with `"auth": "password"` (its own password), `"credential"` (a saved credential, by its ID; the host takes the credential's user) or `"no_password"` (the SSH agent and the default keys).
- `route` is `{"type": "direct"}`, `{"type": "jump", "hosts": ["<host-id>", ...]}` or `{"type": "proxy", "kind": "http", "host": "...", "port": 3128}`.
- A credential is `"kind": "password"`, `"key"` (with `key_path`, a key file on this machine read once into MicaTerm's vault, or `private_key`, the key's text) or `"agent"`.
- `hosts delete` leaves the host's open cards in MicaTerm as they are. `credentials delete` makes its hosts log in on their own; the reply lists them.

## Windows

The same commands work in PowerShell, cmd, and Git Bash. Local paths may be Windows paths (`.\dist`, `C:\Users\me\app.tar.gz`); remote paths are still POSIX paths.

In Git Bash, set `MSYS_NO_PATHCONV=1` before a command whose remote path starts with `/`: otherwise Git Bash rewrites `/srv/app` into a Windows path such as `C:/Program Files/Git/srv/app` before `micaterm` sees it.

```bash
MSYS_NO_PATHCONV=1 micaterm upload <host-id> ./dist /srv/app
```

In PowerShell, prefer `exec --json`: build the request as an object, pipe it in, and read the result as JSON. Nothing in the command needs quoting, and the result is plain ASCII, so the console's code page cannot garble remote output:

```powershell
@{ host = '<host-id>'; command = 'grep -c "error" /var/log/app.log' } | ConvertTo-Json -Compress | micaterm exec --json | ConvertFrom-Json
```

`exec --json` reads `{"host": "<host-id>", "command": "<command>"}` from stdin and, once the command ends, prints `{"exit_code": N, "stdout": "...", "stderr": "..."}`, or `{"error": {"code": "...", "message": "..."}}` when it could not run it. Windows PowerShell 5.1 pipes text to programs as ASCII: write non-ASCII characters in the command as `\uXXXX` escapes.

To create or update a host or credential from Windows PowerShell 5.1, pipe the JSON in the same way, writing non-ASCII characters in it as `\uXXXX` escapes:

```powershell
@{ name = 'web-01'; host = '10.0.1.12'; user = 'deploy' } | ConvertTo-Json -Compress | micaterm hosts create
```

## Rules

- Do not ask the user for SSH passwords, private keys, or passphrases. MicaTerm holds those credentials.
- Only when the user gives you a password, key or passphrase to save in MicaTerm, put it in the JSON on stdin (`password`, `private_key`, `passphrase`), never in a command argument. MicaTerm keeps it in its vault and never prints it back: `show` only says whether one is saved.
- Create, change or delete hosts and credentials only when the user asks you to.
- `micaterm exec` opens a connection of its own, runs the command, and closes the connection. Its stdout and stderr are the remote command's, and its exit code is the remote command's exit code.
- The remote command gets no stdin, so interactive programs (editors, pagers, password prompts) will not work. Use non-interactive flags such as `--yes`, `-y`, or `--no-pager`, and `sudo -n`.
- Long-running tasks should use tmux, nohup, or systemd on the remote host.
- Destination paths follow scp: if the destination exists and is a directory, the source keeps its name inside that directory; otherwise the destination path is the final file or folder path, and its parent directory must exist.

## Errors

Errors are printed to stderr with a code in brackets, such as `[not_enabled]`. Exit code 255 means `micaterm` could not run the command at all.

- `not_running`: MicaTerm is not open. Ask the user to open MicaTerm.
- `not_enabled`: ask the user to turn on Enable external Agent (启用外部 Agent) in MicaTerm under Settings → Agent (设置 → Agent).
- `vault_locked`: MicaTerm's vault is locked, and the login or the change needs a secret in it. MicaTerm shows the user a notice to unlock it. Ask the user to unlock the vault in MicaTerm, then try once more; do not retry in a loop.
- `vault_unavailable`: a change would save a secret, and MicaTerm has no vault that can take it yet. Tell the user the message.
- `host_not_found`: run `micaterm hosts list` again and use an `id` from its output. `credential_not_found`: the same with `micaterm credentials list`.
- `bad_request` from `create` or `update`: the message says which field is wrong, in the words the MicaTerm forms use.
- `save_failed`: the change was made, but not all of it could be written to disk or the vault. Tell the user the message.
- `version_mismatch`: the `micaterm` command and the running MicaTerm are different versions. Ask the user to restart MicaTerm, or to update the command in Settings → Agent.
- `host_key_unknown`, `missing_credential`: MicaTerm has not connected to this host yet, or has no saved password. Ask the user to connect to the host once in MicaTerm and save the password, or to hand you a card connected to it (see Cards Handed to You).
- `host_key_changed`: the server's host key changed. Tell the user; do not try to work around it.
- `transfer_failed`: a transfer could not start or did not finish. Tell the user the message.
- `no_terminal` (cards): no handed card has this ID: it was taken back or closed. Run `micaterm cards list`; when the card is not there, ask the user to hand it to you again.
- `not_connected` (cards): the card is not connected. Ask the user to reconnect it.
- `terminal_busy` (cards): a full-screen program has the card, another `exec --card` command still runs there, the shell did not answer, or it did not start the command (a bastion host's command filter may hold it back). Look with `cards read`; quit a full-screen program with `cards send-key` only when the user wants you to, else tell the user the message.
- `unsupported_shell` (`exec --card`): the card runs a shell other than bash, dash or busybox ash. Tell the user.
- `too_long` (`exec --card`): split the command into shorter ones.
- `connect_failed` saying there is no permission to connect to MicaTerm: the agent's sandbox blocks local inter-process communication. Ask the user to allow it, or run the command outside the sandbox.
