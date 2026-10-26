package server

// BaseModel provides common model functionality.
type BaseModel struct {
	ID   int
	Name string
}

// ExtendedModel embeds BaseModel and adds logging.
type ExtendedModel struct {
	BaseModel
	*Logger
	Extra string
}

// Logger handles structured logging.
type Logger struct {
	Level string
}

type config struct {
	host string
	port int
}
