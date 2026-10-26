package main

import "fmt"

func main() {
	fmt.Println(greet("world"))
}

func greet(name string) string {
	return fmt.Sprintf("Hello, %s!", name)
}
