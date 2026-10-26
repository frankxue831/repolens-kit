package server

// Server handles incoming connections.
type Server struct {
	Addr string
}

// Handler defines the request handling interface.
type Handler interface {
	Handle(req string) error
}

// NewServer creates a new Server instance.
func NewServer(addr string) *Server {
	return &Server{Addr: addr}
}

// Start begins listening on the server address.
func (s *Server) Start() error {
	return nil
}

// Stop gracefully shuts down the server.
func (s Server) Stop() error {
	return nil
}
