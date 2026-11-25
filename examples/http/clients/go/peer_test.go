package ledgerclient

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"
)

type object = map[string]any

func read(t *testing.T, path string) []byte {
	t.Helper()
	b, e := os.ReadFile(path)
	if e != nil {
		t.Fatal(e)
	}
	return b
}
func decode(t *testing.T, b []byte, v any) {
	t.Helper()
	if e := json.Unmarshal(b, v); e != nil {
		t.Fatal(e)
	}
}
func digest(b []byte) string { return fmt.Sprintf("%x", sha256.Sum256(b)) }
func report(t *testing.T, v any) {
	t.Helper()
	b, e := json.Marshal(v)
	if e != nil {
		t.Fatal(e)
	}
	if e = os.WriteFile(os.Getenv("EVIDENCE"), b, 0600); e != nil {
		t.Fatal(e)
	}
}
func accountValue(a Account) object {
	return object{"id": a.ID, "owner": a.Owner, "balance": strconv.FormatInt(a.Balance, 10)}
}
func native(r Result) object {
	m := object{"kind": r.Kind}
	switch v := r.Value.(type) {
	case Account:
		m["value"] = accountValue(v)
	case Pair:
		m["value"] = object{"source": accountValue(v.Source), "destination": accountValue(v.Destination)}
	}
	if r.Error != "" {
		m["error"] = r.Error
	}
	return m
}

type script struct {
	CaseID, Operation, Role string
	Action                  struct {
		Input   map[string]string
		Replies []object
	}
	Expected []object
}

func comparable(t *testing.T, obs []object) []object {
	b, e := json.Marshal(obs)
	if e != nil {
		t.Fatal(e)
	}
	var out []object
	decode(t, b, &out)
	for _, o := range out {
		req, ok := o["request"].(map[string]any)
		if !ok {
			continue
		}
		parts := strings.Split(req["path"].(string), "/")
		for i, s := range parts {
			parts[i], e = url.PathUnescape(s)
			if e != nil {
				t.Fatal(e)
			}
		}
		req["path"] = parts
		if req["contentType"] == "application/json" {
			data, e := base64.StdEncoding.DecodeString(req["bodyBase64"].(string))
			if e != nil {
				t.Fatal(e)
			}
			var v any
			if json.Unmarshal(data, &v) == nil {
				req["bodyBase64"] = v
			}
		}
	}
	return out
}
func runCase(t *testing.T, c script) object {
	var mu sync.Mutex
	observations := []object{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		if len(observations) > len(c.Action.Replies) {
			t.Error("too many outgoing exchanges")
			http.Error(w, "unexpected request", 400)
			return
		}
		data, e := io.ReadAll(r.Body)
		if e != nil {
			t.Error(e)
			return
		}
		var media any
		if h := r.Header.Values("Content-Type"); len(h) > 0 {
			media = h[0]
		}
		request := object{"method": r.Method, "path": r.RequestURI, "contentType": media, "bodyBase64": base64.StdEncoding.EncodeToString(data)}
		obs := object{"request": request, "response": nil}
		index := len(observations)
		if index >= len(c.Action.Replies) || c.Action.Replies[index] == nil {
			conn, _, e := w.(http.Hijacker).Hijack()
			if e != nil {
				t.Error(e)
				return
			}
			observations = append(observations, obs)
			conn.Close()
			return
		}
		reply := c.Action.Replies[index]
		if s, ok := reply["contentType"].(string); ok {
			w.Header().Set("Content-Type", s)
		} else {
			w.Header()["Content-Type"] = nil
		}
		status := int(reply["status"].(float64))
		w.WriteHeader(status)
		body, e := base64.StdEncoding.DecodeString(reply["bodyBase64"].(string))
		if e != nil {
			t.Error(e)
			return
		}
		n, e := w.Write(body)
		if e != nil {
			t.Error(e)
			return
		}
		var ct any
		if values := w.Header().Values("Content-Type"); len(values) > 0 {
			ct = values[0]
		}
		obs["response"] = object{"status": status, "contentType": ct, "bodyBase64": base64.StdEncoding.EncodeToString(body[:n])}
		observations = append(observations, obs)
	}))
	httpClient := server.Client()
	httpClient.Timeout = 5 * time.Second
	client := Client{BaseURL: server.URL, HTTP: httpClient}
	input := c.Action.Input
	var result Result
	switch c.Operation {
	case "urn:oddlang:ledger-http:LookupAccount":
		result = client.Lookup(input["id"])
	case "urn:oddlang:ledger-http:TransferFunds":
		n, e := strconv.ParseInt(input["amount"], 10, 64)
		if e != nil {
			t.Fatal(e)
		}
		result = client.Transfer(input["source"], input["destination"], n)
	default:
		t.Fatalf("unsupported operation %s", c.Operation)
	}
	server.Close()
	mu.Lock()
	defer mu.Unlock()
	observations = append(observations, object{"native": native(result)})
	outcome := "pass"
	if !reflect.DeepEqual(comparable(t, observations), comparable(t, c.Expected)) {
		outcome = "fail"
	}
	return object{"caseId": c.CaseID, "outcome": outcome, "observations": observations}
}
func TestConformance(t *testing.T) {
	bundle := os.Getenv("BUNDLE")
	if bundle == "" {
		t.Fatal("set BUNDLE, PIN, CLAIM and EVIDENCE")
	}
	var lock struct{ Pins []map[string]string }
	decode(t, read(t, os.Getenv("PIN")), &lock)
	root, e := os.OpenRoot(bundle)
	if e != nil {
		t.Fatal(e)
	}
	defer root.Close()
	bytes, e := root.ReadFile("manifest.json")
	if e != nil {
		t.Fatal(e)
	}
	var pin map[string]string
	for _, p := range lock.Pins {
		if p["manifestSha256"] == digest(bytes) {
			pin = p
			break
		}
	}
	if pin == nil {
		t.Fatal("manifest digest mismatch")
	}
	var m struct {
		Identity, Version             string
		SchemaVersion, RuleSetVersion int
		Files                         map[string]string
	}
	decode(t, bytes, &m)
	if m.Identity != pin["identity"] || m.Version != pin["version"] || m.SchemaVersion != 1 || m.RuleSetVersion != 1 {
		t.Fatal("manifest mismatch")
	}
	var cases []byte
	for path, hash := range m.Files {
		if !filepath.IsLocal(path) || filepath.Clean(path) != path || strings.Contains(path, "\\") {
			t.Fatalf("invalid payload path %s", path)
		}
		data, e := root.ReadFile(path)
		if e != nil {
			t.Fatal(e)
		}
		if digest(data) != hash {
			t.Fatalf("payload digest mismatch: %s", path)
		}
		if path == "cases.json" {
			cases = data
		}
	}
	var corpus []script
	decode(t, cases, &corpus)
	var claim struct {
		SchemaVersion int
		Claims        []map[string]string
	}
	decode(t, read(t, os.Getenv("CLAIM")), &claim)
	supported := map[string]bool{"urn:oddlang:ledger-http:LookupAccount": true, "urn:oddlang:ledger-http:TransferFunds": true}
	if claim.SchemaVersion != 1 || len(claim.Claims) == 0 {
		t.Fatal("invalid consumer claim")
	}
	for _, p := range claim.Claims {
		if p["role"] != "http.consumer" || !supported[p["operation"]] {
			t.Fatal("invalid consumer claim")
		}
		delete(supported, p["operation"])
	}
	if addr := os.Getenv("SMOKE"); addr != "" {
		report(t, native(Client{BaseURL: addr}.Lookup("A")))
		return
	}
	results := []object{}
	for _, c := range corpus {
		for _, p := range claim.Claims {
			if c.Role == p["role"] && c.Operation == p["operation"] {
				result := runCase(t, c)
				results = append(results, result)
				if result["outcome"] != "pass" {
					t.Errorf("failed observations %s", c.CaseID)
				}
				break
			}
		}
	}
	report(t, object{"schemaVersion": 1, "release": pin, "ruleSetVersion": 1, "claims": claim.Claims, "results": results})
}

func TestBaseURLPrefix(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.RequestURI != "/api/accounts/A" {
			t.Errorf("unexpected target %q", r.RequestURI)
		}
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{"id":"A","owner":"Alice","balance":"7"}`)
	}))
	defer server.Close()
	for _, suffix := range []string{"/api", "/api/"} {
		result := Client{BaseURL: server.URL + suffix, HTTP: server.Client()}.Lookup("A")
		if result.Kind != "success" {
			t.Errorf("base prefix %q: %s", suffix, result.Kind)
		}
	}
}
