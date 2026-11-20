import {
  Column,
  CreateDateColumn,
  DeleteDateColumn,
  Entity,
  Index,
  PrimaryGeneratedColumn,
  UpdateDateColumn,
} from 'typeorm';
import { Exclude } from 'class-transformer';

export enum MerchantStatus {
  ACTIVE = 'active',
  INACTIVE = 'inactive',
  SUSPENDED = 'suspended',
}

export enum MerchantRole {
  MERCHANT = 'merchant',
  ADMIN = 'admin',
}

@Entity('merchants')
export class Merchant {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Index({ unique: true })
  @Column({ type: 'varchar', length: 255 })
  email: string;

  @Exclude()
  @Column({ type: 'varchar', length: 255, name: 'password_hash' })
  passwordHash: string;

  @Column({ type: 'varchar', length: 255, name: 'business_name' })
  businessName: string;

  @Column({ type: 'varchar', length: 100, name: 'business_type' })
  businessType: string;

  @Column({ type: 'varchar', length: 2 })
  country: string;

  @Column({
    type: 'enum',
    enum: MerchantStatus,
    default: MerchantStatus.ACTIVE,
  })
  status: MerchantStatus;

  @Column({
    type: 'enum',
    enum: MerchantRole,
    default: MerchantRole.MERCHANT,
  })
  role: MerchantRole;

  @Exclude()
  @Column({ type: 'varchar', length: 255, name: 'api_key_hash', nullable: true })
  apiKeyHash: string | null;

  @Column({
    type: 'decimal',
    precision: 5,
    scale: 4,
    default: 0.015,
    transformer: {
      to: (value: number) => value,
      from: (value: string) => parseFloat(value),
    },
  })
  feeRate: number;

  @Column({ type: 'boolean', name: 'sandbox_mode', default: false })
  sandboxMode: boolean;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;

  @UpdateDateColumn({ name: 'updated_at' })
  updatedAt: Date;

  @DeleteDateColumn({ name: 'deleted_at', nullable: true })
  deletedAt: Date | null;
}
