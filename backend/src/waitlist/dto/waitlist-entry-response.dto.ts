export class WaitlistEntryResponseDto {
  id: string;
  email: string;
  username: string | null;
  referralCode: string | null;
  unsubscribed: boolean;
  createdAt: Date;
  updatedAt: Date;
}
