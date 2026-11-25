package client

import (
	"encoding/json"
	"math"
	"os"
	"reflect"
	"testing"
)

func TestPublishedVectors(t *testing.T) {
	version, raw, err := verifiedVectors()
	if err != nil {
		t.Fatal(err)
	}
	if version != os.Getenv("EXPECTED_VERSION") {
		t.Fatalf("unexpected pin %q", version)
	}
	var v struct {
		Version  string
		Account  json.RawMessage
		Transfer struct {
			BehaviorVersion       string
			State                 []Account
			Request, ErrorRequest Request
			Success, Error        Result
		}
	}
	if err := json.Unmarshal(raw, &v); err != nil {
		t.Fatal(err)
	}
	if v.Version != version || v.Transfer.BehaviorVersion != "v1" {
		t.Fatal("vector version mismatch")
	}
	account, err := decodeAccount(version, v.Account)
	if err != nil || int64(account.Balance) != math.MaxInt64 || account.ID != "max" || account.Owner != "Alice" {
		t.Fatalf("account %+v: %v", account, err)
	}
	if (version == "v2" && account.CurrencyCode != "CAD") || (version == "v1" && account.CurrencyCode != "") {
		t.Fatal("currency projection")
	}
	f := v.Transfer
	if got := transfer(f.State, f.Request); !reflect.DeepEqual(got, f.Success) {
		t.Fatalf("success %+v, expected %+v", got, f.Success)
	}
	if got := transfer(f.State, f.ErrorRequest); !reflect.DeepEqual(got, f.Error) {
		t.Fatalf("failure %+v, expected %+v", got, f.Error)
	}
}
