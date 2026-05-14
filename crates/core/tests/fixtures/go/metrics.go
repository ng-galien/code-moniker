package metrics

import (
	"context"
	"io"
	fmtx "fmt"
	. "strings"
	_ "github.com/lib/pq"

	"github.com/prometheus/client_golang/prometheus"
)

const (
	StatusOK    = 200
	StatusError = 500
	defaultTag  = "global"
)

var (
	ErrTimeout  = fmtx.Errorf("timeout")
	defaultBuf  = make([]byte, 0, 1024)
	moduleStart int64
	reqCounter  = prometheus.NewCounter(prometheus.CounterOpts{Name: "reqs"})
)

type (
	Counter    int64
	Histogram  = []float64
	BucketSpec struct {
		Lower float64
		Upper float64
	}
	AnyBucket = BucketSpec
	Snapshot  interface {
		Read() error
	}
)

type Labels map[string]string

type Reader interface {
	io.Reader
	Read(p []byte) (int, error)
}

type Sampler struct {
	prometheus.Collector
	*Counter
	Labels Labels
}

type ledger struct {
	entries []Counter
}

type Pipeline struct {
	Sampler
	src   Reader
	sinks []io.Writer
	cfg   map[string]Counter
}

func newPipeline(src Reader, sinks ...io.Writer) *Pipeline {
	return &Pipeline{src: src, sinks: sinks, cfg: map[string]Counter{}}
}

func (p *Pipeline) Push(ctx context.Context, name string, vals ...float64) (Counter, *BucketSpec, error) {
	id := ToUpper(name)
	bucket := &BucketSpec{Lower: 0, Upper: 1}
	p.cfg[id] = Counter(len(vals))
	merged := newSampler(p).labels()
	_ = merged
	return p.cfg[id], bucket, nil
}

func (p *Pipeline) Drain(out *Counter, route map[Counter]string) {
	for k, v := range route {
		if v == "" {
			continue
		}
		*out += k
	}
}

func (p *Pipeline) snapshot(specs []*BucketSpec) *ledger {
	l := &ledger{entries: make([]Counter, 0, len(specs))}
	for _, s := range specs {
		_ = s
	}
	return l
}

func newSampler(p *Pipeline) *Sampler {
	return &Sampler{Labels: Labels{"pipeline": defaultTag}}
}

func (s *Sampler) labels() Labels {
	return s.Labels
}

func Tally(a, b int, marks []float64) (*BucketSpec, error) {
	hist := prometheus.HistogramOpts{Name: "tally"}
	spec := &BucketSpec{Lower: float64(a), Upper: float64(b)}
	_ = hist
	_ = marks
	return spec, nil
}
