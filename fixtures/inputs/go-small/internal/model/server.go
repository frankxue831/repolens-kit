package model

// BaseServer provides common server fields via embedding.
type BaseServer struct {
	Addr string
}

// Server embeds BaseServer and adds TLS configuration.
type Server struct {
	BaseServer
	TLSEnabled bool
}

// Handler is the interface for request handlers.
type Handler interface {
	Handle(req string) string
}

// NewServer constructs a Server with the given address.
func NewServer(addr string) *Server {
	return &Server{BaseServer: BaseServer{Addr: addr}}
}

// Start starts the server.
func (s *Server) Start() {
	_ = s.Addr
}

// stop is an unexported method.
func (s *server) stop() {}

// unexportedHelper is a package-private function.
func unexportedHelper() {}
