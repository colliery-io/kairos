// Package leftpad is third-party code, copied in.
package leftpad

import "strings"

// Pad adds spaces on the left until s has width characters.
func Pad(s string, width int) string {
	if len(s) >= width {
		return s
	}
	return strings.Repeat(" ", width-len(s)) + s
}
