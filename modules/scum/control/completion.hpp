#pragma once
#include <cstdint>
#include <map>
#include <stdexcept>
#include <vector>

namespace scum_save {
// Protected by the observer mutex. Native worker publication can race ahead of
// its C++ return: a main continuation may finish before batch_returned(). These
// independent debts reflect that actual ordering rather than forcing a false
// linear "queue empty => done" state.
class Completion {
    struct Task { bool entered{},returned{},captured{},settled{}; };
    std::map<std::int32_t,Task> tasks_;
    unsigned stages_{},batches_{},pumps_{};
    bool root_returned_{},failed_{},unknown_{};
    const char* issue_{"native_save_pending"};
    void need(bool value,const char* issue) {
        if(!value){unknown(issue);throw std::runtime_error(issue);}
    }
public:
    // Later cleanup, foreign SQL or timeout events must not hide the first rejection.
    void failed(const char* issue) noexcept {if(!rejected())issue_=issue;failed_=true;}
    void unknown(const char* issue) noexcept {if(!rejected())issue_=issue;unknown_=true;}
    const char* issue() const noexcept{return issue_;}
    bool rejected() const noexcept{return failed_||unknown_;}
    bool failed() const noexcept{return failed_;}
    void stage(unsigned stage) {
        need(stage==1||stage==2||stage==4,"native_stage_unknown");
        need(!(stages_&stage)&&!root_returned_,"native_stage_repeated");stages_|=stage;
    }
    void published(std::int32_t sequence) {
        need(sequence>0&&tasks_.size()<262144,"native_queue_sequence");
        need(tasks_.try_emplace(sequence).second,"native_queue_sequence_reused");
    }
    void cancelled(std::int32_t sequence) noexcept {
        if(tasks_.contains(sequence))unknown("native_task_cancelled");
    }
    void root_returned() {need(!root_returned_&&stages_==7&&!tasks_.empty(),"native_root_incomplete");root_returned_=true;}
    void batch_entered(const std::vector<std::int32_t>& sequences) {
        need(!sequences.empty()&&batches_==0,"native_batch_nested");
        for(const auto sequence:sequences) {
            const auto found=tasks_.find(sequence);
            need(found!=tasks_.end()&&!found->second.entered,"native_batch_foreign_sequence");
            found->second.entered=true;
        }
        ++batches_;
    }
    void batch_returned(const std::vector<std::int32_t>& sequences) {
        need(batches_==1,"native_batch_not_entered");
        for(const auto sequence:sequences) {
            const auto found=tasks_.find(sequence);
            need(found!=tasks_.end()&&found->second.entered&&!found->second.returned,"native_batch_return_sequence");
            found->second.returned=true;
        }
        --batches_;
    }
    void pump_entered(const std::vector<std::int32_t>& sequences) {
        need(pumps_==0,"native_pump_nested");
        for(const auto sequence:sequences) {
            const auto found=tasks_.find(sequence);
            need(found!=tasks_.end()&&found->second.entered&&!found->second.captured,"native_pump_foreign_sequence");
            found->second.captured=true;
        }
        ++pumps_;
    }
    void pump_returned(const std::vector<std::int32_t>& sequences) {
        need(pumps_==1,"native_pump_not_entered");
        for(const auto sequence:sequences) {
            const auto found=tasks_.find(sequence);
            need(found!=tasks_.end()&&found->second.captured&&!found->second.settled,"native_pump_return_sequence");
            found->second.settled=true;
        }
        --pumps_;
    }
    bool tasks_settled() const noexcept {
        if(batches_||pumps_||tasks_.empty())return false;
        for(const auto& [sequence,task]:tasks_) {
            (void)sequence;if(!task.entered||!task.returned||!task.captured||!task.settled)return false;
        }
        return true;
    }
    bool native_settled() const noexcept {return root_returned_&&tasks_settled();}
    bool confirmed(bool sql_and_io_closed,bool readback_matches) const noexcept {
        return !rejected()&&native_settled()&&sql_and_io_closed&&readback_matches;
    }
};
}
