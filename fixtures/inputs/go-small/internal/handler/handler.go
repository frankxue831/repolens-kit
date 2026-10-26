package handler

import (
	"go-small/internal/model"
)

// Handler wraps a Server with pointer embedding.
type Handler struct {
	*model.Server
}

// NewHandler creates a Handler for the given server.
func NewHandler(s *model.Server) *Handler {
	return &Handler{Server: s}
}

// Start delegates to the embedded Server.
func (h *Handler) Start() {
	h.Server.Start()
}
