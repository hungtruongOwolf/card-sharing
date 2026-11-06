import { useMemo } from 'react';

import { useAuthClient } from '../auth';
import { createCardsApi, type CardsApi } from './cardsApi';

/** The card API over the signed-in auth client (one instance per auth client). */
export function useCardsApi(): CardsApi {
  const authClient = useAuthClient();
  return useMemo(() => createCardsApi(authClient.fetch), [authClient]);
}
