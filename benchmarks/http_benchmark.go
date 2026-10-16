// Go net/http benchmark reference server for TruLang comparison
package main

import (
	"encoding/json"
	"net/http"
	"runtime"
)

type ResponsePayload struct {
	Status       string `json:"status"`
	Server       string `json:"server"`
	User         string `json:"user"`
	RegionAReset string `json:"region_a_reset"`
}

func main() {
	runtime.GOMAXPROCS(runtime.NumCPU())

	http.HandleFunc("/ping", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("pong"))
	})

	http.HandleFunc("/api/v1/users", func(w http.ResponseWriter, r *http.Request) {
		userID := r.URL.Query().Get("id")
		payload := ResponsePayload{
			Status:       "success",
			Server:       "go-net-http",
			User:         userID,
			RegionAReset: "GC-dependent",
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		json.NewEncoder(w).Encode(payload)
	})

	http.ListenAndServe(":8081", nil)
}
