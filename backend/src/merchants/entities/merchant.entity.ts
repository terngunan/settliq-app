import {
  Column,
  CreateDateColumn,
  Entity,
  Index,
  PrimaryGeneratedColumn,
  UpdateDateColumn,
} from 'typeorm';

export enum MerchantRole {
  MERCHANT = 'merchant',
  ADMIN = 'admin',
  SUPERADMIN = 'superadmin',
}

export enum MerchantStatus {
  PENDING = 'pending',
  ACTIVE = 'active',
  SUSPENDED = 'suspended',
}

@Entity('merchants')
export class Merchant {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ unique: true })
  email: string;

  @Column()
  name: string;

  @Column({ nullable: true })
  passwordHash?: string;

  @Column({ nullable: true, select: false })
  passwordResetToken?: string;

  @Column({ type: 'timestamptz', nullable: true })
  passwordResetExpiry?: Date;

  @Column({ nullable: true, select: false })
  apiKeyHash?: string;

  @Index('IDX_merchants_apiKeyLookupHash')
  @Column({ nullable: true, select: false })
  apiKeyLookupHash?: string;

  @Column({ nullable: true })
  apiKeyLastFour?: string;

  @Column({ type: 'timestamptz', nullable: true })
  apiKeyCreatedAt?: Date;

  @Column({ type: 'timestamptz', nullable: true })
  apiKeyRevokedAt?: Date;

  @Column({
    type: 'enum',
    enum: MerchantStatus,
    default: MerchantStatus.PENDING,
  })
  status: MerchantStatus;

  @Column({ type: 'enum', enum: MerchantRole, default: MerchantRole.MERCHANT })
  role: MerchantRole;

  @CreateDateColumn()
  createdAt: Date;

  @UpdateDateColumn()
  updatedAt: Date;
}
