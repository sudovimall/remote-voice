import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { spawn } from "node:child_process";

const baseURL = new URL(process.env.PLAYWRIGHT_BASE_URL ?? "http://127.0.0.1:18080");
const port = baseURL.port || (baseURL.protocol === "https:" ? "443" : "80");
const workDir = "/tmp/remote-voice-playwright-auth";
const databasePath = `${workDir}/remote-voice-${process.pid}.db`;
const configPath = `${workDir}/application-${process.pid}.yaml`;

mkdirSync(workDir, { recursive: true });
rmSync(databasePath, { force: true });
rmSync(`${databasePath}-shm`, { force: true });
rmSync(`${databasePath}-wal`, { force: true });

writeFileSync(
  configPath,
  `port: ${port}
room:
  max_members: 8
  disconnect_grace_seconds: 30
media:
  udp_port_min: 40000
  udp_port_max: 40100
screen_share:
  max_width: 1280
  max_height: 720
  max_frame_rate: 24
auth:
  enabled: true
  admin:
    username: admin
    password_hash: "$argon2id$v=19$m=19456,t=2,p=1$TRSWC792aAkah2sJk8lSGw$PO0c7VIUwwGwRFvwpcx4DryFRWkqP/7LFfcV2p9Hqw0"
    display_name: 管理员
  session:
    cookie_name: remote_voice_session
    ttl_hours: 168
    secure: never
storage:
  kind: sqlite
  sqlite:
    path: ${databasePath}
`,
  "utf8",
);

const child = spawn("cargo", ["run"], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    REMOTE_VOICE_CONFIG: configPath,
  },
  stdio: "inherit",
});

function shutdown() {
  child.kill("SIGTERM");
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

child.on("exit", (code, signal) => {
  rmSync(configPath, { force: true });
  process.exit(code ?? (signal ? 1 : 0));
});
