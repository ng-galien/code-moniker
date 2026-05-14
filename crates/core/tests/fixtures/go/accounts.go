// Package store contains the user-account persistence layer.
//
// The repository abstraction lets the HTTP handlers stay decoupled from
// the concrete storage. The in-memory implementation here is used by tests
// and by the local dev server; production wires the same interface to
// pgxpool in cmd/server/main.go.
package store

import (
	"errors"
	"fmt"
	"strings"
	"sync"
)

// User is the canonical record. Fields are tag-less because the JSON
// envelope is built by the HTTP layer, not here.
type User struct {
	ID    string
	Email string
	Name  string
	Tags  []string
}

// ErrNotFound is returned by lookup methods when no row matches.
// Callers should compare with errors.Is, not ==.
var ErrNotFound = errors.New("not found")

// ConflictError signals a uniqueness violation. The fields are exported so
// HTTP middleware can render structured 409 bodies.
type ConflictError struct {
	Field string
	Value string
}

// Error implements the error interface.
func (e *ConflictError) Error() string {
	return fmt.Sprintf("conflict on %s=%q", e.Field, e.Value)
}

// UserRepository is the contract every storage backend must satisfy.
//
// Implementations are expected to be safe for concurrent use; the HTTP
// runtime calls from many goroutines.
type UserRepository interface {
	FindByID(id string) (*User, error)
	FindByEmail(email string) (*User, error)
	Insert(user User) (*User, error)
	// Scan iterates over every user. Order is unspecified.
	Scan() <-chan User
}

// InMemory is a sync.RWMutex-backed reference implementation. Suitable
// for tests and local dev only — there is no persistence across restarts.
type InMemory struct {
	mu    sync.RWMutex
	users map[string]User
}

// NewInMemory builds an empty store.
func NewInMemory() *InMemory {
	return &InMemory{users: make(map[string]User)}
}

func (m *InMemory) FindByID(id string) (*User, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	u, ok := m.users[id]
	if !ok {
		return nil, ErrNotFound
	}
	// Return a copy so callers can't mutate our internal state.
	return &u, nil
}

func (m *InMemory) FindByEmail(email string) (*User, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	for _, u := range m.users {
		if u.Email == email {
			return &u, nil
		}
	}
	return nil, ErrNotFound
}

func (m *InMemory) Insert(user User) (*User, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	// Linear uniqueness check is fine for the in-memory impl. Postgres
	// impl relies on the UNIQUE index on (email).
	for _, existing := range m.users {
		if existing.Email == user.Email {
			return nil, &ConflictError{Field: "email", Value: user.Email}
		}
	}
	m.users[user.ID] = user
	return &user, nil
}

// Scan ships every user over a channel that closes when iteration finishes.
// The producer goroutine releases the read lock as soon as it returns; the
// consumer drives back-pressure naturally by not reading.
func (m *InMemory) Scan() <-chan User {
	out := make(chan User)
	go func() {
		defer close(out)
		m.mu.RLock()
		defer m.mu.RUnlock()
		for _, u := range m.users {
			out <- u
		}
	}()
	return out
}

// WithTag returns every user wearing tag.
//
// TODO(perf): switch to a tag-index when the user count grows past O(10k);
// linear scan is currently fine for our small tenants.
func WithTag(repo UserRepository, tag string) []User {
	var result []User
	for u := range repo.Scan() {
		for _, t := range u.Tags {
			if t == tag {
				result = append(result, u)
				break
			}
		}
	}
	return result
}

// MakeID returns the local part of the email, lowercased. Falls back to
// the full address when no '@' is present (defensive).
func MakeID(email string) string {
	at := strings.IndexByte(email, '@')
	if at <= 0 {
		return strings.ToLower(email)
	}
	return strings.ToLower(email[:at])
}
