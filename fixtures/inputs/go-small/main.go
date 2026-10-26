package main

import (
	"fmt"
	"gosmall/server"
)

func main() {
	s := server.NewServer("localhost:8080")
	s.Start()
	fmt.Println("running")
}

func setup() string {
	return "ready"
}
