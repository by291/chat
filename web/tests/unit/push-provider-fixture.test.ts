import { createECDH, ECDH, type ECDHKeyFormat } from 'node:crypto';
import { readFile, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';

import { afterAll, afterEach, expect, test, vi } from 'vitest';

import { testDirectory } from '../../../tests/infrastructure.mts';
import { TestPushProvider } from '../integration/push-support';

vi.mock('../../../tests/infrastructure.mts', async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  const { mkdtemp } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const directory = await mkdtemp(join(tmpdir(), 'chat-push-fixture-'));

  // Isolate the fixture root so every test starts before middleware creates it.
  return { ...original, testDirectory: join(directory, 'tests') };
});

afterEach(async () => {
  vi.restoreAllMocks();
  await rm(testDirectory, { recursive: true, force: true });
});

afterAll(() =>
  rm(path.dirname(testDirectory), { recursive: true, force: true }),
);

test('the TLS provider starts before middleware creates the test root and cleans up only its own directory', async () => {
  const provider = new TestPushProvider();
  const unrelatedFile = path.join(testDirectory, 'other-fixture.txt');

  try {
    await provider.start();
    const certificate = provider.environment().CHAT_TEST_PUSH_CA_FILE;
    expect((await stat(certificate)).isFile()).toBe(true);
    await writeFile(unrelatedFile, 'preserve');

    await provider.stop();

    await expect(stat(certificate)).rejects.toMatchObject({ code: 'ENOENT' });
    expect(await readFile(unrelatedFile, 'utf8')).toBe('preserve');
  } finally {
    await provider.stop();
  }
});

test('the TLS provider exports a fixed-width P-256 scalar when Node omits leading zeros', async () => {
  const privateKey = Buffer.alloc(32);
  privateKey[31] = 1;
  vi.spyOn(ECDH.prototype, 'generateKeys').mockImplementation(function (
    this: ECDH,
    encoding: BufferEncoding,
    format?: ECDHKeyFormat,
  ) {
    this.setPrivateKey(privateKey);
    return this.getPublicKey(encoding, format);
  });
  const provider = new TestPushProvider();

  try {
    await provider.start();
    const config = provider.environment();
    const encoded = Buffer.from(config.WEB_PUSH_PRIVATE_KEY, 'base64url');

    expect(encoded).toEqual(privateKey);
    const decoded = createECDH('prime256v1');
    decoded.setPrivateKey(encoded);
    expect(decoded.getPublicKey().toString('base64url')).toBe(
      config.WEB_PUSH_PUBLIC_KEY,
    );
  } finally {
    await provider.stop();
  }
});
