import { createResource, createSignal, onCleanup } from "solid-js";

const POLL_INTERVAL = 30;

export function usePolling<T>(fetcher: () => Promise<T>) {
  const [lastPolled, setLastPolled] = createSignal<Date | null>(null);
  const [countdown, setCountdown] = createSignal(POLL_INTERVAL);

  const wrappedFetcher = async () => {
    const result = await fetcher();
    setLastPolled(new Date());
    setCountdown(POLL_INTERVAL);
    return result;
  };

  const [data, { refetch }] = createResource(wrappedFetcher);

  const timer = setInterval(() => {
    setCountdown((prev) => {
      if (prev <= 1) {
        refetch();
        return POLL_INTERVAL;
      }
      return prev - 1;
    });
  }, 1000);

  onCleanup(() => clearInterval(timer));

  const manualRefetch = () => {
    refetch();
  };

  return {
    data,
    refetch: manualRefetch,
    lastPolled,
    countdown,
  } as const;
}
