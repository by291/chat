/** Owned HTTPS provider with independent Node VAPID verification and RFC 8291 decryption. */
import {
  createECDH,
  createDecipheriv,
  createPrivateKey,
  createPublicKey,
  hkdfSync,
  randomBytes,
  verify,
  X509Certificate,
  type ECDH,
} from 'node:crypto';
import { createServer, type Server } from 'node:https';
import type { Socket } from 'node:net';
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import path from 'node:path';

import { testDirectory } from '../../../tests/infrastructure.mts';

export type PushFixtureEnvironment = Record<
  | 'WEB_PUSH_ENABLED'
  | 'WEB_PUSH_PRIVATE_KEY'
  | 'WEB_PUSH_PUBLIC_KEY'
  | 'WEB_PUSH_CONTACT'
  | 'WEB_PUSH_ALLOWED_HOSTS'
  | 'CHAT_TEST_PUSH_ADDRESS'
  | 'CHAT_TEST_PUSH_HOST'
  | 'CHAT_TEST_PUSH_CA_FILE',
  string
>;
export type TestPushSubscription = {
  endpoint: string;
  p256dh: string;
  auth: string;
};
export type CapturedPush = {
  endpoint: string;
  status: number;
  payload: unknown;
  vapidValid: boolean;
};

export class TestPushProvider {
  private server?: Server;
  private directory?: string;
  private sockets = new Set<Socket>();
  private captured: CapturedPush[] = [];
  private recipients = new Map<string, { key: ECDH; auth: Buffer }>();
  private vapid = createECDH('prime256v1');
  private status = 201;

  constructor() {
    this.vapid.generateKeys();
  }

  async start(): Promise<void> {
    // Provider fixtures start before TestRuntime initializes shared middleware.
    await mkdir(testDirectory, { recursive: true });
    this.directory = await mkdtemp(path.join(testDirectory, 'push-'));
    try {
      const certificate = await readFile(
        new URL('../fixtures/push/certificate.der', import.meta.url),
      );
      const key = await readFile(
        new URL('../fixtures/push/key.der', import.meta.url),
      );
      await writeFile(
        path.join(this.directory, 'certificate.der'),
        certificate,
      );
      this.server = createServer(
        {
          cert: new X509Certificate(certificate).toString(),
          key: createPrivateKey({ key, format: 'der', type: 'pkcs8' }).export({
            format: 'pem',
            type: 'pkcs8',
          }),
        },
        (request, response) => {
          const chunks: Buffer[] = [];
          let size = 0;
          request.on('data', (chunk: Buffer) => {
            size += chunk.length;
            if (size > 16384) request.destroy();
            else chunks.push(chunk);
          });
          request.on('error', () => response.destroy());
          request.on('end', () => {
            try {
              const endpoint = `https://push.example.test${request.url}`;
              const recipient = this.recipients.get(endpoint);
              if (
                !recipient ||
                request.headers['content-encoding'] !== 'aes128gcm'
              )
                throw new Error('Invalid fixture request');
              const payload = this.decrypt(Buffer.concat(chunks), recipient);
              const vapidValid = this.verifyVapid(
                request.headers.authorization ?? '',
              );
              this.captured.push({
                endpoint,
                status: this.status,
                payload,
                vapidValid,
              });
              response.writeHead(vapidValid ? this.status : 403).end();
            } catch {
              // Invalid fixture crypto never becomes a successful provider acceptance.
              response.writeHead(400).end();
            }
          });
        },
      );
      this.server.on('connection', (socket) => {
        this.sockets.add(socket);
        socket.once('close', () => this.sockets.delete(socket));
        socket.on('error', () => socket.destroy());
      });
      await new Promise<void>((resolve, reject) => {
        this.server!.once('error', reject);
        this.server!.listen(0, '127.0.0.1', () => {
          this.server!.off('error', reject);
          resolve();
        });
      });
    } catch (error) {
      await this.stop();
      throw error;
    }
  }

  environment(): PushFixtureEnvironment {
    const address = this.server?.address();
    if (!address || typeof address === 'string' || !this.directory)
      throw new Error('Push fixture is not running');
    // Node's ECDH scalar export drops leading zeros; VAPID configuration uses
    // the fixed 32-byte P-256 representation, without changing its value.
    const privateKey = Buffer.alloc(32);
    const scalar = this.vapid.getPrivateKey();
    scalar.copy(privateKey, privateKey.length - scalar.length);

    return {
      WEB_PUSH_ENABLED: 'true',
      WEB_PUSH_PRIVATE_KEY: privateKey.toString('base64url'),
      WEB_PUSH_PUBLIC_KEY: this.vapid.getPublicKey().toString('base64url'),
      WEB_PUSH_CONTACT: 'mailto:operator@example.test',
      WEB_PUSH_ALLOWED_HOSTS: 'push.example.test',
      CHAT_TEST_PUSH_ADDRESS: `127.0.0.1:${address.port}`,
      CHAT_TEST_PUSH_HOST: 'push.example.test',
      CHAT_TEST_PUSH_CA_FILE: path.join(this.directory, 'certificate.der'),
    };
  }

  subscription(name: string): TestPushSubscription {
    const endpoint = `https://push.example.test/${encodeURIComponent(name)}`;
    let recipient = this.recipients.get(endpoint);
    if (!recipient) {
      const key = createECDH('prime256v1');
      key.generateKeys();
      recipient = { key, auth: randomBytes(16) };
      this.recipients.set(endpoint, recipient);
    }
    return {
      endpoint,
      p256dh: recipient.key.getPublicKey().toString('base64url'),
      auth: recipient.auth.toString('base64url'),
    };
  }

  requests(): CapturedPush[] {
    return structuredClone(this.captured);
  }

  async stop(): Promise<void> {
    for (const socket of this.sockets) socket.destroy();
    if (this.server?.listening)
      await new Promise<void>((resolve) => this.server!.close(() => resolve()));
    if (this.directory) {
      const owned = path.resolve(this.directory);
      if (!owned.startsWith(path.resolve(testDirectory) + path.sep))
        throw new Error('Unowned push fixture path');
      await rm(owned, { recursive: true, force: true });
    }
    this.directory = undefined;
    this.captured = [];
    this.recipients.clear();
  }

  private verifyVapid(authorization: string): boolean {
    const match = /^vapid t=([^, ]+), k=([^, ]+)$/.exec(authorization);
    if (!match || match[2] !== this.vapid.getPublicKey().toString('base64url'))
      return false;
    const [header, claims, signature] = match[1].split('.');
    const metadata = JSON.parse(Buffer.from(header, 'base64url').toString());
    const payload = JSON.parse(Buffer.from(claims, 'base64url').toString());
    const bytes = this.vapid.getPublicKey();
    const key = createPublicKey({
      key: {
        kty: 'EC',
        crv: 'P-256',
        x: bytes.subarray(1, 33).toString('base64url'),
        y: bytes.subarray(33).toString('base64url'),
      },
      format: 'jwk',
    });
    return (
      metadata.alg === 'ES256' &&
      payload.aud === 'https://push.example.test' &&
      payload.sub === 'mailto:operator@example.test' &&
      payload.exp > Date.now() / 1000 &&
      payload.exp <= Date.now() / 1000 + 86400 &&
      verify(
        'sha256',
        Buffer.from(`${header}.${claims}`),
        { key, dsaEncoding: 'ieee-p1363' },
        Buffer.from(signature, 'base64url'),
      )
    );
  }

  private decrypt(
    body: Buffer,
    recipient: { key: ECDH; auth: Buffer },
  ): unknown {
    if (body.length < 103 || body[20] !== 65)
      throw new Error('Invalid encrypted record');
    const salt = body.subarray(0, 16);
    const publicKey = body.subarray(21, 86);
    const shared = recipient.key.computeSecret(publicKey);
    // RFC 8291 section 3.4, independently implemented only in this receiver fixture.
    const info = Buffer.concat([
      Buffer.from('WebPush: info\0'),
      recipient.key.getPublicKey(),
      publicKey,
    ]);
    const ikm = Buffer.from(
      hkdfSync('sha256', shared, recipient.auth, info, 32),
    );
    const key = Buffer.from(
      hkdfSync(
        'sha256',
        ikm,
        salt,
        Buffer.from('Content-Encoding: aes128gcm\0'),
        16,
      ),
    );
    const nonce = Buffer.from(
      hkdfSync(
        'sha256',
        ikm,
        salt,
        Buffer.from('Content-Encoding: nonce\0'),
        12,
      ),
    );
    const ciphertext = body.subarray(86);
    if (body.readUInt32BE(16) < ciphertext.length)
      throw new Error(
        'Multiple records are unsupported by this bounded fixture',
      );
    const decipher = createDecipheriv('aes-128-gcm', key, nonce);
    decipher.setAuthTag(ciphertext.subarray(-16));
    const plaintext = Buffer.concat([
      decipher.update(ciphertext.subarray(0, -16)),
      decipher.final(),
    ]);
    let delimiter = plaintext.length - 1;
    while (plaintext[delimiter] === 0) delimiter--;
    if (plaintext[delimiter] !== 2)
      throw new Error('Missing final encrypted record delimiter');
    return JSON.parse(plaintext.subarray(0, delimiter).toString());
  }
}
