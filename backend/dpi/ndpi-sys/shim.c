/* Field accessors compiled against the real nDPI headers, so Rust never
 * depends on bindgen's layout of ndpi_flow_struct (packed bitfields). */
#include <ndpi_api.h>

int tmon_ndpi_flow_confidence(const struct ndpi_flow_struct *flow) {
  return (int)flow->confidence;
}

const char *tmon_ndpi_flow_host(const struct ndpi_flow_struct *flow) {
  return flow->host_server_name;
}

unsigned int tmon_ndpi_flow_host_len(void) {
  return (unsigned int)sizeof(((struct ndpi_flow_struct *)0)->host_server_name);
}
