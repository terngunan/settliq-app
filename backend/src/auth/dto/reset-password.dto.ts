import { ApiProperty, PickType } from '@nestjs/swagger';
import { IsString, MinLength } from 'class-validator';
import { RegisterDto } from './register.dto';

export class ResetPasswordDto extends PickType(RegisterDto, ['password'] as const) {
  @ApiProperty()
  @IsString()
  @MinLength(1)
  token: string;
}
