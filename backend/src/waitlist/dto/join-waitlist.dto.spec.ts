import { plainToInstance } from 'class-transformer';
import { validate } from 'class-validator';
import { JoinWaitlistDto } from './join-waitlist.dto';

describe('JoinWaitlistDto', () => {
  const buildDto = (payload: Record<string, unknown>) =>
    plainToInstance(JoinWaitlistDto, payload);

  const usernameErrors = async (username: string) => {
    const dto = buildDto({ email: 'user@example.com', username });
    const errors = await validate(dto);
    return errors.filter((error) => error.property === 'username');
  };

  it('accepts valid alphanumeric and underscore usernames', async () => {
    expect(await usernameErrors('acme_corp')).toHaveLength(0);
    expect(await usernameErrors('abc')).toHaveLength(0);
    expect(await usernameErrors('a'.repeat(30))).toHaveLength(0);
  });

  it('rejects usernames shorter than 3 characters', async () => {
    expect(await usernameErrors('ab')).not.toHaveLength(0);
  });

  it('rejects usernames longer than 30 characters', async () => {
    expect(await usernameErrors('a'.repeat(31))).not.toHaveLength(0);
  });

  it('rejects usernames containing whitespace', async () => {
    expect(await usernameErrors('acme corp')).not.toHaveLength(0);
  });

  it('rejects usernames containing punctuation or slashes', async () => {
    expect(await usernameErrors('acme-corp')).not.toHaveLength(0);
    expect(await usernameErrors('acme/corp')).not.toHaveLength(0);
  });

  it('rejects usernames containing emojis', async () => {
    expect(await usernameErrors('acme😀')).not.toHaveLength(0);
  });

  it('allows username to be omitted', async () => {
    const dto = buildDto({ email: 'user@example.com' });
    const errors = await validate(dto);
    expect(errors.filter((error) => error.property === 'username')).toHaveLength(0);
  });
});
