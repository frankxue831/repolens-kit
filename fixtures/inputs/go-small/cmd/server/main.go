package main

import (
	"go-small/internal/handler"
	"go-small/internal/model"
)

func main() {
	s := model.NewServer("localhost:8080")
	h := handler.NewHandler(s)
	h.Start()
}
