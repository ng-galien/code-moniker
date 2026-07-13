package webrouter

import (
	"fmt"
	"strings"
)

func Build() string {
	r := NewRouter()
	rt := r.HandleFunc("/users")
	fmt.Println(rt.Path())
	return strings.ToUpper(rt.Path())
}
