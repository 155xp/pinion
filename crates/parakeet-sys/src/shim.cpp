// A narrow C surface over parakeet.h, so Rust never mirrors its structs.
#include "parakeet.h"
#include <cstdlib>
#include <cstring>
#include <string>

static void pk_quiet(enum ggml_log_level, const char *, void *) {}

extern "C" {

void * pk_load(const char * path, int use_gpu) {
    parakeet_log_set(pk_quiet, nullptr);
    parakeet_context_params p = parakeet_context_default_params();
    p.use_gpu = use_gpu != 0;
    return parakeet_init_from_file_with_params(path, p);
}

// Transcribes 16 kHz mono samples. Returns UTF-8 text to free with pk_free_text, or null.
char * pk_transcribe(void * ctx, const float * samples, int n_samples, int n_threads) {
    auto * c = static_cast<parakeet_context *>(ctx);
    parakeet_full_params fp = parakeet_full_default_params(PARAKEET_SAMPLING_GREEDY);
    fp.n_threads = n_threads;
    if (parakeet_full(c, fp, samples, n_samples) != 0) {
        return nullptr;
    }
    std::string out;
    const int n = parakeet_full_n_segments(c);
    for (int i = 0; i < n; i++) {
        const char * t = parakeet_full_get_segment_text(c, i);
        if (!t) continue;
        while (*t == ' ') t++;
        if (!*t) continue;
        if (!out.empty()) out += ' ';
        out += t;
    }
    char * res = static_cast<char *>(std::malloc(out.size() + 1));
    if (!res) return nullptr;
    std::memcpy(res, out.c_str(), out.size() + 1);
    return res;
}

void pk_free_text(char * text) { std::free(text); }

void pk_free(void * ctx) { parakeet_free(static_cast<parakeet_context *>(ctx)); }

const char * pk_system_info(void) { return parakeet_print_system_info(); }

}
