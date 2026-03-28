import { Subject, Observable, filter, share } from 'rxjs';

export interface BusEvent<T = unknown> {
  type: string;
  payload: T;
  timestamp: number;
  correlationId?: string;
}

export type EventHandler<T> = (event: BusEvent<T>) => void | Promise<void>;

export class EventBus {
  private subject = new Subject<BusEvent>();
  private stream$ = this.subject.asObservable().pipe(share());

  publish<T>(type: string, payload: T, correlationId?: string): void {
    this.subject.next({ type, payload, timestamp: Date.now(), correlationId });
  }

  on<T>(eventType: string): Observable<BusEvent<T>> {
    return this.stream$.pipe(
      filter((e) => e.type === eventType),
    ) as Observable<BusEvent<T>>;
  }

  subscribe<T>(eventType: string, handler: EventHandler<T>): () => void {
    const sub = this.on<T>(eventType).subscribe((e) => handler(e));
    return () => sub.unsubscribe();
  }

  once<T>(eventType: string): Promise<BusEvent<T>> {
    return new Promise((resolve) => {
      const unsub = this.subscribe<T>(eventType, (e) => { resolve(e); unsub(); });
    });
  }

  complete(): void {
    this.subject.complete();
  }
}
