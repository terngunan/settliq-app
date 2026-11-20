import { ApiProperty } from '@nestjs/swagger';
import { IsEmail, IsString, Matches, MaxLength, MinLength } from 'class-validator';

export class RegisterDto {
  @ApiProperty({ example: 'Acme Store' })
  @IsString()
  @MinLength(1)
  @MaxLength(255)
  name: string;

  @ApiProperty({ example: 'user@example.com' })
  @IsEmail()
  email: string;

  @ApiProperty({ example: 'SecurePass123!', minLength: 8, maxLength: 72 })
  @IsString()
  @MinLength(8)
  @MaxLength(72)
  @Matches(/((?=.*\d)|(?=.*\W+))(?![.\n])(?=.*[A-Z])(?=.*[a-z]).*$/, {
    message:
      'password must contain at least one uppercase letter, one lowercase letter, and one number or special character',
  })
  password: string;
}
