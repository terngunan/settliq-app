import { SetMetadata } from '@nestjs/common';

export enum MerchantRole {
  ADMIN = 'admin',
  MERCHANT = 'merchant',
}

export const ROLES_KEY = 'roles';
export const Roles = (...roles: MerchantRole[]) => SetMetadata(ROLES_KEY, roles);
