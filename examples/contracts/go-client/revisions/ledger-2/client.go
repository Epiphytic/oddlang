package client

import (
	"encoding/json"
	"fmt"
	"regexp"
	"slices"
	"strconv"
	"unicode/utf8"
)

type Decimal int64

func (d Decimal) MarshalJSON() ([]byte, error) {
	return json.Marshal(strconv.FormatInt(int64(d), 10))
}

func (d *Decimal) UnmarshalJSON(raw []byte) error {
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return err
	}
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil || strconv.FormatInt(n, 10) != s {
		return fmt.Errorf("invalid decimal %q", s)
	}
	*d = Decimal(n)
	return nil
}

type decimalRule struct {
	Min, Max Decimal
	Pattern  string
}

func (r decimalRule) check(d Decimal) error {
	match, err := regexp.MatchString(r.Pattern, strconv.FormatInt(int64(d), 10))
	if err != nil || !match || d < r.Min || d > r.Max {
		return fmt.Errorf("decimal %d outside wire rules", d)
	}
	return nil
}

type Wire struct {
	AccountFields map[string]string
	Encoding      struct {
		Balance, Amount decimalRule
		CurrencyCode    struct{ MinLength int }
	}
	Transfer *struct{ ErrorsByPriority []string }
}
type Account struct {
	ID, Owner, CurrencyCode string
	Balance                 Decimal
}

func decodeAccount(w Wire, raw []byte) (Account, error) {
	var fields map[string]json.RawMessage
	var a Account
	if err := json.Unmarshal(raw, &fields); err != nil {
		return a, err
	}
	targets := map[string]any{"urn:oddlang:ledger:id": &a.ID, "urn:oddlang:ledger:owner": &a.Owner, "urn:oddlang:ledger:balance": &a.Balance, "urn:oddlang:ledger:currencyCode": &a.CurrencyCode}
	for iri, key := range w.AccountFields {
		target, ok := targets[iri]
		if !ok {
			return a, fmt.Errorf("unknown account field %s", iri)
		}
		b := fields[key]
		if len(b) == 0 || string(b) == "null" {
			return a, fmt.Errorf("missing %s", key)
		}
		if err := json.Unmarshal(b, target); err != nil {
			return a, fmt.Errorf("%s: %w", key, err)
		}
	}
	if utf8.RuneCountInString(a.CurrencyCode) < w.Encoding.CurrencyCode.MinLength {
		return a, fmt.Errorf("invalid currencyCode")
	}
	return a, w.Encoding.Balance.check(a.Balance)
}

type Request struct {
	Source      string  `json:"source"`
	Destination string  `json:"destination"`
	Amount      Decimal `json:"amount"`
}
type Result struct {
	OK       bool
	Balances map[string]Decimal
	Error    string
}

func decodeReply(w Wire, request Request, raw []byte) (Result, error) {
	var r Result
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return r, err
	}
	if w.Transfer == nil {
		return r, fmt.Errorf("release does not define transfer")
	}
	if b, ok := fields["error"]; ok {
		if len(fields) != 1 || json.Unmarshal(b, &r.Error) != nil || !slices.Contains(w.Transfer.ErrorsByPriority, r.Error) {
			return r, fmt.Errorf("invalid transfer error")
		}
		return r, nil
	}
	if len(fields) != 2 || json.Unmarshal(fields["ok"], &r.OK) != nil || !r.OK || json.Unmarshal(fields["balances"], &r.Balances) != nil {
		return r, fmt.Errorf("invalid success payload")
	}
	if len(r.Balances) != 2 {
		return r, fmt.Errorf("expected both affected balances")
	}
	for _, id := range []string{request.Source, request.Destination} {
		d, ok := r.Balances[id]
		if !ok {
			return r, fmt.Errorf("missing balance for %s", id)
		}
		if err := w.Encoding.Balance.check(d); err != nil {
			return r, err
		}
	}
	return r, nil
}

// submit sends the native request to the caller's transport and validates its wire response.
func submit(w Wire, request Request, send func([]byte) ([]byte, error)) (Result, error) {
	if w.Transfer == nil {
		return Result{}, fmt.Errorf("release does not define transfer")
	}
	if err := w.Encoding.Amount.check(request.Amount); err != nil {
		return Result{}, err
	}
	raw, err := json.Marshal(request)
	if err != nil {
		return Result{}, err
	}
	response, err := send(raw)
	if err != nil {
		return Result{}, err
	}
	return decodeReply(w, request, response)
}
