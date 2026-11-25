// Package ledgerclient is an independent implementation of the v1 HTTP ledger.
package ledgerclient

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"
)

type Account struct {
	ID      string
	Owner   string
	Balance int64
}
type Pair struct{ Source, Destination Account }
type Result struct {
	Kind  string
	Value any
	Error string
}
type Client struct {
	BaseURL string
	HTTP    *http.Client
}

func failure() Result { return Result{Kind: "protocolError"} }
func (c Client) Lookup(id string) Result {
	if id == "" || id == "." || id == ".." || !utf8.ValidString(id) {
		return Result{Kind: "localRejection"}
	}
	return c.call("GET", "/accounts/"+url.PathEscape(id), nil)
}
func (c Client) Transfer(source, destination string, amount int64) Result {
	if !utf8.ValidString(source) || !utf8.ValidString(destination) {
		return Result{Kind: "localRejection"}
	}
	body, err := json.Marshal(map[string]string{"source": source, "destination": destination, "amount": strconv.FormatInt(amount, 10)})
	if err != nil {
		return Result{Kind: "localRejection"}
	}
	return c.call("POST", "/transfers", body)
}

// encoding/json replaces isolated UTF-16 surrogates. Validate escape pairs
// before tokenization, skipping escaped backslashes so literal text stays intact.
func validEscapes(data []byte) bool {
	for i := 0; i < len(data); i++ {
		if data[i] != '\\' {
			continue
		}
		i++
		if i >= len(data) {
			return false
		}
		if data[i] != 'u' {
			continue
		}
		if i+4 >= len(data) {
			return false
		}
		n, e := strconv.ParseUint(string(data[i+1:i+5]), 16, 16)
		if e != nil {
			return false
		}
		i += 4
		if n >= 0xdc00 && n <= 0xdfff {
			return false
		}
		if n >= 0xd800 && n <= 0xdbff {
			if i+6 >= len(data) || string(data[i+1:i+3]) != "\\u" {
				return false
			}
			low, e := strconv.ParseUint(string(data[i+3:i+7]), 16, 16)
			if e != nil || low < 0xdc00 || low > 0xdfff {
				return false
			}
			i += 6
		}
	}
	return true
}
func value(d *json.Decoder) (any, error) {
	tok, err := d.Token()
	if err != nil {
		return nil, err
	}
	switch t := tok.(type) {
	case string, bool:
		return t, nil
	case json.Delim:
		if t == '{' {
			obj := map[string]any{}
			for d.More() {
				key, err := d.Token()
				if err != nil {
					return nil, err
				}
				name, ok := key.(string)
				if !ok {
					return nil, fmt.Errorf("key")
				}
				if _, exists := obj[name]; exists {
					return nil, fmt.Errorf("duplicate key")
				}
				v, err := value(d)
				if err != nil {
					return nil, err
				}
				obj[name] = v
			}
			end, err := d.Token()
			if err == nil && end == json.Delim('}') {
				return obj, nil
			}
		}
	}
	return nil, fmt.Errorf("invalid value")
}
func account(v any) (Account, bool) {
	m, ok := v.(map[string]any)
	if !ok || len(m) != 3 {
		return Account{}, false
	}
	id, i := m["id"].(string)
	owner, o := m["owner"].(string)
	s, b := m["balance"].(string)
	n, e := strconv.ParseInt(s, 10, 64)
	if !i || !o || !b || e != nil || s != strconv.FormatInt(n, 10) || n < 0 {
		return Account{}, false
	}
	return Account{id, owner, n}, true
}

var jsonMedia = regexp.MustCompile(`(?i)^[ \t]*application/json(?:[ \t]*;[ \t]*charset[ \t]*=[ \t]*(?:utf-8|"utf-8"))?[ \t]*$`)

var lookupErrors = map[string]int{"urn:oddlang:http:error:InvalidRequest": 400, "urn:oddlang:http:error:AccountMissing": 404}
var transferErrors = map[string]int{
	"urn:oddlang:http:error:InvalidRequest": 400, "urn:oddlang:http:error:UnsupportedMediaType": 415,
	"urn:oddlang:ledger:error:SourceMissing": 404, "urn:oddlang:ledger:error:DestinationMissing": 404,
	"urn:oddlang:ledger:error:SameAccount": 422, "urn:oddlang:ledger:error:NonPositiveAmount": 422,
	"urn:oddlang:ledger:error:InsufficientFunds": 409, "urn:oddlang:ledger:error:DestinationOverflow": 409,
}

func (c Client) call(method, path string, body []byte) Result {
	base, e := url.Parse(c.BaseURL)
	if e != nil || base.RawQuery != "" || base.Fragment != "" {
		return failure()
	}
	req, e := http.NewRequest(method, strings.TrimRight(c.BaseURL, "/")+path, bytes.NewReader(body))
	if e != nil {
		return failure()
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	transport := c.HTTP
	if transport == nil {
		transport = http.DefaultClient
	}
	// A redirect is itself an undeclared response.
	client := *transport
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	r, e := client.Do(req)
	if e != nil {
		return failure()
	}
	defer r.Body.Close()
	if len(r.Header.Values("Content-Type")) != 1 || !jsonMedia.MatchString(r.Header.Get("Content-Type")) {
		return failure()
	}
	data, e := io.ReadAll(r.Body)
	if e != nil || !utf8.Valid(data) || !validEscapes(data) {
		return failure()
	}
	d := json.NewDecoder(bytes.NewReader(data))
	obj, e := value(d)
	if e != nil {
		return failure()
	}
	if _, e = d.Token(); e != io.EOF {
		return failure()
	}
	m, ok := obj.(map[string]any)
	if !ok {
		return failure()
	}
	if r.StatusCode != 200 {
		iri, ok := m["error"].(string)
		errors := lookupErrors
		if method == "POST" {
			errors = transferErrors
		}
		if !ok || len(m) != 1 || errors[iri] != r.StatusCode {
			return failure()
		}
		return Result{Kind: "namedError", Error: iri}
	}
	if method == "GET" {
		a, ok := account(m)
		if ok {
			return Result{Kind: "success", Value: a}
		}
	} else if len(m) == 2 {
		a, x := account(m["source"])
		b, y := account(m["destination"])
		if x && y {
			return Result{Kind: "success", Value: Pair{a, b}}
		}
	}
	return failure()
}
