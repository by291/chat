/** Initialize development credentials once; existing configuration is never replaced. */
import { randomBytes } from "node:crypto";
import { readFile, writeFile, access, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const envFile = new URL(".env", root);
const storageFile = new URL("infra/seaweedfs/s3.json", root);
const existingFiles = [];

for (const file of [envFile, storageFile]) {
  try {
    await access(file);
  } catch (error) {
    if (error.code === "ENOENT") continue;
    throw error;
  }
  existingFiles.push(file);
}

if (existingFiles.length === 2) {
  console.log(
    "Development credentials are already initialized; keeping existing files.",
  );
  process.exit(0);
}
if (existingFiles.length === 1) {
  console.error(
    `Incomplete configuration: ${fileURLToPath(existingFiles[0])} exists, but its companion file is missing. Restore the missing file with matching credentials; no files were changed.`,
  );
  process.exit(1);
}

const secret = () => randomBytes(32).toString("hex");
const databasePassword = secret();
const storageSecret = secret();
const template = await readFile(new URL(".env.example", root), "utf8");
const env = template
  .replaceAll(
    "postgres://chat:CHANGE_ME",
    `postgres://chat:${databasePassword}`,
  )
  .replaceAll(
    "POSTGRES_PASSWORD=CHANGE_ME",
    `POSTGRES_PASSWORD=${databasePassword}`,
  )
  .replaceAll("CENTRIFUGO_API_KEY=CHANGE_ME", `CENTRIFUGO_API_KEY=${secret()}`)
  .replaceAll("INTERNAL_PROXY_KEY=CHANGE_ME", `INTERNAL_PROXY_KEY=${secret()}`)
  .replaceAll(
    "SEAWEED_SECRET_KEY=CHANGE_ME",
    `SEAWEED_SECRET_KEY=${storageSecret}`,
  );
const storage = {
  identities: [
    {
      name: "chat",
      credentials: [{ accessKey: "chat-local", secretKey: storageSecret }],
      actions: ["Admin", "Read", "Write", "List", "Tagging"],
    },
  ],
};

// Git does not retain the empty directory when the local credentials are ignored.
await mkdir(new URL("./", storageFile), { recursive: true });
await writeFile(storageFile, JSON.stringify(storage, null, 2), {
  flag: "wx",
  mode: 0o600,
});
await writeFile(envFile, env, { flag: "wx", mode: 0o600 });
console.log(
  "Created development credentials in .env and infra/seaweedfs/s3.json.",
);
