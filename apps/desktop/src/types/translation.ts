export interface TranslationProviderDisclosure {
  providerId: string;
  displayName: string;
  sendsDataOffDevice: boolean;
}

export interface TranslationReviewRow {
  id: string;
  sourceText: string;
  translatedText: string;
  lockedNames: string[];
}
