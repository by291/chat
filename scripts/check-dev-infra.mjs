/** Fail before background startup when this checkout's infrastructure is not ready. */
import { spawnSync } from "node:child_process";

const composeArguments = ["compose", ...process.argv.slice(2)];

function compose(...args) {
  const result = spawnSync("docker", [...composeArguments, ...args], {
    encoding: "utf8",
    windowsHide: true,
    timeout: 15_000,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      "Cannot inspect Docker dependencies. Check Docker, then run make infra.",
    );
  }
  return result.stdout.trim();
}

try {
  const services = compose("config", "--services").split(/\r?\n/);
  const output = compose("ps", "--all", "--format", "json");
  // Compose versions emit either one JSON array or newline-delimited records.
  const containers = output.startsWith("[")
    ? JSON.parse(output)
    : output
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => JSON.parse(line));
  const unavailable = services.filter((service) => {
    const instances = containers.filter(
      (container) => container.Service === service,
    );
    return (
      !instances.length ||
      instances.some(
        (container) =>
          container.State !== "running" ||
          (container.Health && container.Health !== "healthy"),
      )
    );
  });
  if (unavailable.length) {
    throw new Error(
      `Dependencies are not ready: ${unavailable.join(", ")}. Run make infra first.`,
    );
  }
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
