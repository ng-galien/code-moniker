package webrouter

type Route struct {
	path string
}

func (rt *Route) Path() string {
	return rt.path
}
