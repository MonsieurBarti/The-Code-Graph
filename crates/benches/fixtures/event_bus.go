package eventbus

import (
	"sync"
	"time"
)

// Event is a typed message on the bus.
type Event struct {
	Type          string
	Payload       interface{}
	Timestamp     time.Time
	CorrelationID string
}

// Handler processes events.
type Handler func(e Event)

// Subscription represents a registered handler.
type Subscription struct {
	id      int
	eventType string
	handler Handler
}

// EventBus is a simple pub/sub in-process event bus.
type EventBus struct {
	mu     sync.RWMutex
	subs   map[string][]*Subscription
	nextID int
}

// New creates a new EventBus.
func New() *EventBus {
	return &EventBus{subs: make(map[string][]*Subscription)}
}

// Subscribe registers a handler for the given event type.
func (b *EventBus) Subscribe(eventType string, h Handler) *Subscription {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.nextID++
	sub := &Subscription{id: b.nextID, eventType: eventType, handler: h}
	b.subs[eventType] = append(b.subs[eventType], sub)
	return sub
}

// Unsubscribe removes a subscription.
func (b *EventBus) Unsubscribe(sub *Subscription) {
	b.mu.Lock()
	defer b.mu.Unlock()
	list := b.subs[sub.eventType]
	for i, s := range list {
		if s.id == sub.id {
			b.subs[sub.eventType] = append(list[:i], list[i+1:]...)
			return
		}
	}
}

// Publish dispatches an event to all matching subscribers.
func (b *EventBus) Publish(eventType string, payload interface{}, correlationID string) {
	e := Event{Type: eventType, Payload: payload, Timestamp: time.Now(), CorrelationID: correlationID}
	b.mu.RLock()
	handlers := append([]*Subscription{}, b.subs[eventType]...)
	b.mu.RUnlock()
	for _, sub := range handlers { sub.handler(e) }
}

// Once subscribes for a single event, then auto-unsubscribes.
func (b *EventBus) Once(eventType string, h Handler) {
	var sub *Subscription
	sub = b.Subscribe(eventType, func(e Event) {
		h(e)
		b.Unsubscribe(sub)
	})
}

// ListenerCount returns the number of handlers for an event type.
func (b *EventBus) ListenerCount(eventType string) int {
	b.mu.RLock()
	defer b.mu.RUnlock()
	return len(b.subs[eventType])
}
