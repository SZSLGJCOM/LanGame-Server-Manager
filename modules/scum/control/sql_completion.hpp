#pragma once
#include <cstdint>

namespace scum_save {
// Kinds come from the real prepared statement: fixed connection +50/+58,
// +60/+68 transaction statements, otherwise sqlite3_stmt_readonly. Callers
// retain each original API result before a later reset can overwrite it.
enum class SqlKind { read,write,begin,commit,rollback };
class SqlCompletion {
    bool failed_{},unknown_{},transaction_{};
    std::uint64_t writes_{},implicit_commits_{},begins_{},commits_{},version_{},verified_{};
public:
    void fail() noexcept {failed_=true;}
    void unknown() noexcept {unknown_=true;}
    void api_result(int result) noexcept {if(result!=0)failed_=true;}
    void step(SqlKind kind,int result,bool autocommit_before,bool autocommit_after) noexcept {
        if(result!=100&&result!=101){failed_=true;return;}
        if(kind==SqlKind::rollback){unknown_=true;return;}
        if(kind==SqlKind::begin) {
            if(result!=101||!autocommit_before||autocommit_after||transaction_)unknown_=true;
            else {transaction_=true;++begins_;}return;
        }
        if(kind==SqlKind::commit) {
            if(result!=101||autocommit_before||!autocommit_after||!transaction_)unknown_=true;
            else {transaction_=false;++commits_;}return;
        }
        if(kind==SqlKind::write) {
            ++writes_;++version_;
            // SCUM's cached mutations have no RETURNING row lifecycle. An
            // unexpected row-producing mutation needs a separate final proof.
            if(result!=101||autocommit_before!=autocommit_after)unknown_=true;
            else if(autocommit_before)++implicit_commits_;
            else if(!transaction_)unknown_=true;
        }
    }
    void readback_verified() noexcept {if(!transaction_&&!failed_&&!unknown_)verified_=version_;}
    bool closed() const noexcept {
        return !failed_&&!unknown_&&!transaction_&&begins_==commits_&&
            (writes_==0||implicit_commits_||commits_)&&verified_==version_;
    }
    bool failed() const noexcept{return failed_;}
    bool unverified() const noexcept{return version_!=verified_;}
};
}
