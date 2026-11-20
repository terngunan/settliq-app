import { plainToInstance } from 'class-transformer';
import { validate } from 'class-validator';
import { RegisterDto } from './register.dto';

describe('RegisterDto', () => {
  it('transforms email to lowercase and trims whitespace', async () => {
    const dto = plainToInstance(RegisterDto, {
      email: '  Merchant@Example.com  ',
      password: 'supersecret',
    });

    expect(dto.email).toBe('merchant@example.com');

    const errors = await validate(dto);
    expect(errors).toHaveLength(0);
  });

  it('rejects an invalid email', async () => {
    const dto = plainToInstance(RegisterDto, {
      email: 'not-an-email',
      password: 'supersecret',
    });

    const errors = await validate(dto);
    expect(errors.some((error) => error.property === 'email')).toBe(true);
  });
});
