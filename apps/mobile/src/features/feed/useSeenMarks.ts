import { useFocusEffect } from 'expo-router';
import { useCallback, useState } from 'react';

import type { SeenMarks, SeenStore } from './seenCards';

/** The seen marks, re-read whenever the screen regains focus (coming back from the story viewer). */
export function useSeenMarks(store: SeenStore): SeenMarks {
  const [marks, setMarks] = useState<SeenMarks>({});
  useFocusEffect(
    useCallback(() => {
      let active = true;
      store.load().then(
        (loaded) => active && setMarks(loaded),
        () => undefined,
      );
      return () => {
        active = false;
      };
    }, [store]),
  );
  return marks;
}
