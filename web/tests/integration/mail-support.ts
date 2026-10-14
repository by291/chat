/** Fixture-owned SMTP capture; messages and configuration stay inside the test run. */
import { randomBytes } from 'node:crypto';
import { createServer, type Server, type Socket } from 'node:net';

export type SmtpFixtureEnvironment = {
  MAIL_ENABLED: string;
  APP_PUBLIC_ORIGIN: string;
  SMTP_HOST: string;
  SMTP_PORT: string;
  SMTP_TLS_MODE: string;
  SMTP_USERNAME: string;
  SMTP_PASSWORD: string;
  SMTP_FROM: string;
  MAIL_QUEUE_ENCRYPTION_KEY: string;
  CHAT_TEST_SMTP_CAPTURE: string;
};

export type CapturedMail = {
  recipient: string;
  subject: string;
  text: string;
  link?: string;
};

export class TestSmtpServer {
  private server: Server;
  private sockets = new Set<Socket>();
  private captured: CapturedMail[] = [];
  private queueKey = randomBytes(32).toString('base64');
  private password = randomBytes(24).toString('base64url');

  constructor() {
    this.server = createServer((socket) => this.capture(socket));
  }

  async start(): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      this.server.once('error', reject);
      this.server.listen(0, '127.0.0.1', () => {
        this.server.off('error', reject);
        resolve();
      });
    });
  }

  environment(): SmtpFixtureEnvironment {
    const address = this.server.address();
    if (!address || typeof address === 'string')
      throw new Error('SMTP fixture is not running');
    return {
      MAIL_ENABLED: 'true',
      APP_PUBLIC_ORIGIN: 'http://localhost:5174',
      SMTP_HOST: '127.0.0.1',
      SMTP_PORT: String(address.port),
      SMTP_TLS_MODE: 'starttls',
      SMTP_USERNAME: 'owned-fixture',
      SMTP_PASSWORD: this.password,
      SMTP_FROM: 'Chat <no-reply@example.test>',
      MAIL_QUEUE_ENCRYPTION_KEY: this.queueKey,
      CHAT_TEST_SMTP_CAPTURE: 'true',
    };
  }

  messages(): CapturedMail[] {
    return this.captured.map((message) => ({ ...message }));
  }

  async stop(): Promise<void> {
    for (const socket of this.sockets) socket.destroy();
    if (this.server.listening)
      await new Promise<void>((resolve) => this.server.close(() => resolve()));
    this.captured = [];
  }

  private capture(socket: Socket) {
    this.sockets.add(socket);
    let buffer = Buffer.alloc(0);
    let body: string[] | undefined;
    let recipient = '';
    socket.on('close', () => this.sockets.delete(socket));
    // Fixture cleanup/peer timeout can close a socket during a write. This server
    // forwards no durable work; parent cleanup owns every connected socket.
    socket.on('error', () => socket.destroy());
    socket.write('220 owned-fixture ESMTP\r\n');
    socket.on('data', (chunk) => {
      buffer = Buffer.concat([
        buffer,
        typeof chunk === 'string' ? Buffer.from(chunk) : chunk,
      ]);
      if (buffer.length > 65536) {
        socket.destroy();
        return;
      }
      let end: number;
      while ((end = buffer.indexOf('\r\n')) !== -1) {
        const line = buffer.subarray(0, end).toString('utf8');
        buffer = buffer.subarray(end + 2);
        if (body) {
          if (line === '.') {
            this.captured.push(parseMail(body.join('\r\n'), recipient));
            body = undefined;
            socket.write('250 accepted\r\n');
          } else {
            body.push(line.startsWith('..') ? line.slice(1) : line);
          }
        } else if (line.startsWith('EHLO')) {
          socket.write(
            '250-owned-fixture\r\n250-8BITMIME\r\n250 AUTH PLAIN\r\n',
          );
        } else if (line.startsWith('AUTH')) {
          socket.write('235 accepted\r\n');
        } else if (line.startsWith('RCPT TO:')) {
          recipient = line
            .slice('RCPT TO:'.length)
            .trim()
            .replace(/^<|>$/g, '');
          socket.write('250 accepted\r\n');
        } else if (line === 'DATA') {
          body = [];
          socket.write('354 send body\r\n');
        } else if (line === 'QUIT') {
          socket.end('221 bye\r\n');
        } else {
          socket.write('250 accepted\r\n');
        }
      }
    });
  }
}

function decodeQuotedPrintable(value: string): string {
  const normalized = value.replace(/=\r?\n/g, '');
  const bytes: number[] = [];
  for (let index = 0; index < normalized.length; index++) {
    const code = normalized.slice(index + 1, index + 3);
    if (normalized[index] === '=' && /^[\da-f]{2}$/i.test(code)) {
      bytes.push(Number.parseInt(code, 16));
      index += 2;
    } else {
      bytes.push(...Buffer.from(normalized[index], 'utf8'));
    }
  }
  return Buffer.from(bytes).toString('utf8');
}

/** Decode the single text/plain MIME messages actually emitted by lettre. */
function parseMail(raw: string, recipient: string): CapturedMail {
  const separator = raw.indexOf('\r\n\r\n');
  if (separator === -1) throw new Error('Malformed fixture mail');
  const headers = raw.slice(0, separator).replace(/\r\n[ \t]+/g, ' ');
  const header = (name: string) =>
    new RegExp(`^${name}: (.*)$`, 'im').exec(headers)?.[1] ?? '';
  const encoded = raw.slice(separator + 4);
  const encoding = header('Content-Transfer-Encoding').toLowerCase();
  const text =
    encoding === 'base64'
      ? Buffer.from(encoded, 'base64').toString('utf8')
      : encoding === 'quoted-printable'
        ? decodeQuotedPrintable(encoded)
        : encoded;
  const subject = header('Subject').replace(
    /=\?utf-8\?([bq])\?([^?]+)\?=/gi,
    (_, kind: string, value: string) =>
      kind.toLowerCase() === 'b'
        ? Buffer.from(value, 'base64').toString('utf8')
        : decodeQuotedPrintable(value.replaceAll('_', ' ')),
  );

  return {
    recipient,
    subject,
    text,
    link: /https?:\/\/[^\s]+/.exec(text)?.[0],
  };
}
