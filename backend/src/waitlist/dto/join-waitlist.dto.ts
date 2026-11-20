import { Transform } from 'class-transformer';
import { IsEmail, IsOptional, IsString, Length, Matches } from 'class-validator';

export class JoinWaitlistDto {
  @IsEmail()
  @Transform(({ value }) => value?.trim().toLowerCase())
  email: string;

  @IsString()
  @Length(3, 30)
  @Matches(/^[a-zA-Z0-9_]+$/, {
    message: 'username may only contain letters, numbers, and underscores',
  })
  @Transform(({ value }) => value?.trim().toLowerCase())
  username: string;

  @IsOptional()
  @IsString()
  @Length(1, 100)
  @Transform(({ value }) => value?.trim())
  businessName?: string;

  @IsOptional()
  @IsString()
  @Length(2, 2)
  @Transform(({ value }) => value?.trim().toUpperCase())
  country?: string;

  @IsOptional()
  @IsString()
  @Transform(({ value }) => value?.trim())
  referralCode?: string;
}
