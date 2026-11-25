package client

import (
	"encoding/json"
	"math"
	"os"
	"reflect"
	"testing"
)

func TestPublishedVectors(t *testing.T) {
	version, wire, raw, err := verifiedInputs()
	if err != nil {
		t.Fatal(err)
	}
	if version != os.Getenv("EXPECTED_VERSION") {
		t.Fatalf("unexpected pin %s", version)
	}
	var vectors struct {
		Version  string
		Account  json.RawMessage
		Transfer *struct{ Request, ErrorRequest, Success, Error json.RawMessage }
	}
	if err = json.Unmarshal(raw, &vectors); err != nil || vectors.Version != version {
		t.Fatalf("invalid vectors: %v", err)
	}
	account, err := decodeAccount(wire, vectors.Account)
	if err != nil || account.Balance != math.MaxInt64 || account.ID != "max" || account.Owner != "Alice" {
		t.Fatalf("invalid account %+v: %v", account, err)
	}
	if version == "v2" {
		if account.CurrencyCode != "CAD" || vectors.Transfer != nil || wire.Transfer != nil {
			t.Fatal("invalid structural release")
		}
		for _, row := range []string{`{"id":"x","owner":"a","balance":"0","currencyCode":""}`, `{"id":"x","owner":"a","balance":"0"}`} {
			if _, err = decodeAccount(wire, []byte(row)); err == nil {
				t.Fatal("accepted missing currency", row)
			}
		}
		return
	}
	f := vectors.Transfer
	if wire.Transfer == nil || f == nil {
		t.Fatal("missing transfer contract")
	}
	for _, tc := range []struct {
		req, reply json.RawMessage
		success    bool
	}{{f.Request, f.Success, true}, {f.ErrorRequest, f.Error, false}} {
		var request Request
		if err = json.Unmarshal(tc.req, &request); err != nil {
			t.Fatal(err)
		}
		got, err := submit(wire, request, func(sent []byte) ([]byte, error) {
			var expected, actual map[string]any
			if err := json.Unmarshal(sent, &actual); err != nil {
				t.Fatal(err)
			}
			if err := json.Unmarshal(tc.req, &expected); err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(expected, actual) {
				t.Fatalf("sent %s, want %s", sent, tc.req)
			}
			return tc.reply, nil
		})
		if err != nil || got.OK != tc.success {
			t.Fatalf("reply %+v: %v", got, err)
		}
		if tc.success && (got.Balances["A"] != 5 || got.Balances["B"] != 5) {
			t.Fatal(got)
		}
		if !tc.success && got.Error != "urn:oddlang:ledger:error:InsufficientFunds" {
			t.Fatal(got)
		}
	}
	for _, bad := range []string{`{"error":"unknown"}`, `{"ok":true,"balances":{"A":"-7","B":"0"}}`, `{"ok":true,"balances":{"A":"abc","B":"0"}}`, `{"ok":true,"balances":{"A":"9223372036854775808","B":"0"}}`, `{"ok":true,"balances":{}}`, `{"ok":true,"balances":null}`, `{"ok":true,"balances":{"A":0,"B":"0"}}`, `{"error":"urn:oddlang:ledger:error:SourceMissing","ok":true}`} {
		if _, err := decodeReply(wire, Request{"A", "B", 4}, []byte(bad)); err == nil {
			t.Fatalf("accepted %s", bad)
		}
	}
}
