// Runs a command with the current environment, for package.json scripts that load `.env` with
// `node --env-file=.env tools/dev/run.mjs <command> [args...]` (Node 22 built-in, no dependency).
// Local development only: shared environments get their variables from the platform.
import { spawnSync } from 'node:child_process';

const [command, ...args] = process.argv.slice(2);
if (!command) {
  console.error('usage: node --env-file=.env tools/dev/run.mjs <command> [args...]');
  process.exit(2);
}
const result = spawnSync(command, args, { stdio: 'inherit' });
if (result.error) {
  console.error(`could not start ${command}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
