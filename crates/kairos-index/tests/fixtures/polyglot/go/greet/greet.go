package greet

import "fmt"

// Greet prints a greeting.
func Greet(name string) {
	fmt.Println(Message(name))
}

// Message builds the text of a greeting.
func Message(name string) string {
	return "Hello, " + name
}

// Greeter greets with a prefix.
type Greeter struct {
	Prefix string
}

// Greet prints a greeting with the prefix.
func (g Greeter) Greet(name string) {
	fmt.Println(g.Prefix + Message(name))
}
