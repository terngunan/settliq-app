import { Global, Module } from '@nestjs/common';
import { JwtModule } from '@nestjs/jwt';
import { PassportModule } from '@nestjs/passport';
import { TypeOrmModule } from '@nestjs/typeorm';
import { ConfigModule, ConfigService } from '@nestjs/config';
import { AuthService } from './auth.service';
import { AuthController } from './auth.controller';
import { JwtStrategy } from './strategies/jwt.strategy';
import { ApiKeyStrategy } from './strategies/api-key.strategy';
import { JwtAuthGuard } from './guards/jwt.guard';
import { ApiKeyGuard } from './guards/api-key.guard';
import { RolesGuard } from './guards/roles.guard';
import { Merchant } from '../merchants/entities/merchant.entity';
import { MerchantApiKeyController } from '../merchants/merchant-api-key.controller';
import { RefreshToken } from './refresh-token.entity';

@Global()
@Module({
  imports: [
    TypeOrmModule.forFeature([Merchant, RefreshToken]),
    PassportModule,
    JwtModule.registerAsync({
      imports: [ConfigModule],
      useFactory: (config: ConfigService) => {
        const secret = config.get<string>('JWT_SECRET');
        if (!secret) {
          throw new Error('JWT_SECRET environment variable is not set. Refusing to start with an insecure fallback.');
        }
        return {
          secret,
          signOptions: { expiresIn: config.get<string>('JWT_EXPIRES_IN', '7d') },
        };
      },
      inject: [ConfigService],
    }),
  ],
  controllers: [AuthController, MerchantApiKeyController],
  providers: [AuthService, JwtStrategy, ApiKeyStrategy, JwtAuthGuard, ApiKeyGuard, RolesGuard],
  exports: [AuthService, JwtAuthGuard, ApiKeyGuard, RolesGuard],
})
export class AuthModule {}
