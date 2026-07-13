package webrouter

import "strings"

type Router struct {
	prefix string
}

func NewRouter() *Router {
	return &Router{}
}

func (r *Router) HandleFunc(path string) *Route {
	return &Route{path: strings.TrimSpace(path)}
}
