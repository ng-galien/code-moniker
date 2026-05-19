package handlers

import (
	"context"
	"encoding/json"
	"net/http"
	"strconv"
)

// cm: def User
type User struct {
	ID    int64  `json:"id"`
	Email string `json:"email"`
}

// cm: def UserStore
type UserStore interface {
	Find(ctx context.Context, id int64) (User, error)
}

// cm: def UserHandler
type UserHandler struct {
	store UserStore
}

func NewUserHandler(store UserStore) *UserHandler {
	return &UserHandler{store: store}
}

// cm: def UserHandler.ServeHTTP
func (h *UserHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	// cm: ref UserHandler.ServeHTTP.calls.parseID
	id, err := parseID(r)
	if err != nil {
		http.Error(w, "bad id", http.StatusBadRequest)
		return
	}
	user, err := h.store.Find(r.Context(), id)
	if err != nil {
		http.Error(w, "missing", http.StatusNotFound)
		return
	}
	_ = json.NewEncoder(w).Encode(user)
}

// cm: def parseID
func parseID(r *http.Request) (int64, error) {
	raw := r.URL.Query().Get("id")
	// cm: ref parseID.calls.strconv.ParseInt
	return strconv.ParseInt(raw, 10, 64)
}
